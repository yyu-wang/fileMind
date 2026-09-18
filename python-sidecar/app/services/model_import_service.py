"""T1 — 离线模型包导入（内网 / 无外网部署的模型分发通道）。

背景：Embedding 与 Rerank 的模型文件是「建立索引」与「知识问答」的硬前置，而在线
获取只有 HF 镜像下载一条路。部署机器在内网 / 无外网时下载必然失败，这台机器上
「检索时没有对应的模型」会让整条问答链路不可用。本模块提供离线通道：把已下载好
模型的机器上的 ``models`` 目录（或其 zip）导入到本机 ``{models_root}``。

包结构、来源识别（zip / 目录）与包内模型探测已按职责拆至
:mod:`app.services.model_import_source`（2026-09-18，原文件 333 行超 Python 警告
阈值 300），本模块保留「暂存 → 校验 → 原子改名」的落盘提交与对外编排。

原子性口径：

- **包内命中但不完整的模型目录 → 整次导入失败**并列出缺失文件。半截目录（有权重、
  少 tokenizer）是打包方的问题，静默忽略会让「导入成功但模型仍不可用」变成谜题；
- 包内未出现的模型不导入；目标机已就绪的模型按幂等跳过（不覆盖可用文件）；
- 写入先落 ``{models_root}/.importing/<model>``，校验齐备后才原子改名到目标目录，
  失败路径清理暂存，半截包不会污染可用的模型目录。
"""

from __future__ import annotations

import asyncio
import contextlib
import os
import shutil
import zipfile
from dataclasses import dataclass, replace
from typing import TYPE_CHECKING

from app.core.logging import getLogger
from app.services.embedding_service import models_root
from app.services.model_download_service import model_ready
from app.services.model_import_source import (
    PackageInvalidError,
    PackageSourceError,
    _detect_packages,
    _ModelPackage,
    _resolve_source,
)
from app.services.model_specs import spec_ready

if TYPE_CHECKING:
    from pathlib import Path

    from app.services.model_specs import DownloadSpec

__all__ = ["ImportOutcome", "PackageInvalidError", "PackageSourceError", "import_package"]

logger = getLogger("filemind.model_import")

#: 暂存目录名（位于 ``models_root`` 下的隐藏目录，导入成功才改名到模型目录）
_STAGING_DIR_NAME = ".importing"

#: 导入串行锁（并发导入会争用同一暂存目录；见 :func:`import_package`）
_import_lock = asyncio.Lock()


@dataclass(frozen=True)
class ImportOutcome:
    """导入结果。

    Attributes:
        imported: 本次写入（或替换）的模型名。
        skipped: 包内已就绪、按幂等跳过的模型名。
    """

    imported: list[str]
    skipped: list[str]


def _rmtree(path: Path) -> None:
    """删除目录（不存在即 no-op）。"""
    with contextlib.suppress(OSError):
        shutil.rmtree(path)


def _staged_ready(staging: Path, spec: DownloadSpec) -> bool:
    """暂存目录是否齐备（复用下载侧同一份就绪口径：存在且非空）。"""
    return spec_ready(replace(spec, root=staging))


def _commit_package(pkg: _ModelPackage, staging_root: Path) -> None:
    """单模型的「暂存 → 校验 → 原子改名」提交。

    目标目录只在暂存区校验通过后才被动过（先删旧的不完整目录再改名），因此
    ``{models_root}/{model}`` 任何时刻要么是旧的不可用状态、要么是新的完整状态。

    Raises:
        PackageSourceError: 读取源文件失败或写入后校验未通过。
    """
    staging = staging_root / pkg.model
    _rmtree(staging)
    try:
        pkg.copy_into(staging)
    except (OSError, zipfile.BadZipFile, KeyError) as exc:
        _rmtree(staging)
        raise PackageSourceError(f"读取模型文件失败（{pkg.model}）: {exc}") from exc
    if not _staged_ready(staging, pkg.spec):
        _rmtree(staging)
        raise PackageSourceError(f"写入后校验未通过（{pkg.model}），请检查磁盘空间后重试")
    _replace_into_place(staging, pkg)
    logger.info("model.import.model", model=pkg.model)


def _replace_into_place(staging: Path, pkg: _ModelPackage) -> None:
    """把校验通过的暂存目录改名到目标位置（尽力先清掉不可用的旧目录）。

    改名是最后一步：任何失败都不应留下「半截模型目录」，也不能让 IO 异常以
    未包装的形式冒泡（路由只认 PackageSourceError，未包装异常会退化成没有
    错误码的 500 响应）。

    Raises:
        PackageSourceError: 目标不可用（同名文件占位 / 权限不足 / 磁盘满）。
    """
    target = pkg.spec.root
    try:
        _rmtree(target)
        target.parent.mkdir(parents=True, exist_ok=True)
        os.replace(staging, target)
    except OSError as exc:
        _rmtree(staging)
        raise PackageSourceError(f"写入目标目录失败（{pkg.model}）: {exc}") from exc


def _import_sync(source: str) -> ImportOutcome:
    """导入主流程（同步，由 :func:`import_package` 下沉线程池）。

    Raises:
        PackageInvalidError: 见 :func:`app.services.model_import_source._detect_packages`。
        PackageSourceError: 见 ``_resolve_source`` / :func:`_commit_package`。
    """
    pkg_source = _resolve_source(source)
    staging_root = models_root() / _STAGING_DIR_NAME
    imported: list[str] = []
    skipped: list[str] = []
    for pkg in _detect_packages(pkg_source):
        if model_ready(pkg.model):
            skipped.append(pkg.model)
            continue
        _commit_package(pkg, staging_root)
        imported.append(pkg.model)
    # 暂存根目录只在本模块使用；清掉使 models_root 保持只有模型目录
    with contextlib.suppress(OSError):
        staging_root.rmdir()
    logger.info("model.import.done", imported=imported, skipped=skipped)
    return ImportOutcome(imported=imported, skipped=skipped)


async def import_package(source: str) -> ImportOutcome:
    """导入离线模型包（阻塞 IO 下沉线程池，不阻塞事件循环）。

    全局串行：两次并发导入会争用同一暂存目录（``.importing/{model}``）与目标目录，
    互相删掉对方的半成品后以 IO 报错收场。本地单用户场景下排队等待的代价可忽略，
    换成正确性更划算（且事件循环仍可服务其他请求）。

    Args:
        source: 离线包路径（zip 文件，或 ``models`` 目录 / 单个模型目录）。

    Returns:
        导入结果（本次写入的模型名 + 已就绪跳过的模型名）。

    Raises:
        PackageInvalidError: 包内无可用模型，或命中的模型目录缺文件。
        PackageSourceError: 路径不存在、包不可读，或落盘失败。
    """
    async with _import_lock:
        return await asyncio.to_thread(_import_sync, source)
