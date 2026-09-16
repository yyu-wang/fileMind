"""T1 — 离线模型包导入（内网 / 无外网部署的模型分发通道）。

背景：Embedding 与 Rerank 的模型文件是「建立索引」与「知识问答」的硬前置，而在线
获取只有 HF 镜像下载一条路。部署机器在内网 / 无外网时下载必然失败，这台机器上
「检索时没有对应的模型」会让整条问答链路不可用。本模块提供离线通道：把已下载好
模型的机器上的 ``models`` 目录（或其 zip）导入到本机 ``{models_root}``。

包结构（就是 ``models`` 目录本身，或它的 zip）：:

    <模型目录名>/<规格声明的文件...>

例如 ``bge-large-zh-v1.5/onnx/model_quantized.onnx``、
``bge-reranker-v2-m3/model.safetensors``。也接受把模型目录套在外层目录里
（``models/bge-large-zh-v1.5/...``）；目录源还额外兼容「用户在文件选择器里直接选中
单个模型目录」——按**父目录**视角解析后，两种选法落到同一套路径规则上。

安全（对齐 ``rules/security.md``）：

- **白名单驱动取文件**：只按 ``resolve_spec`` 声明的清单逐个取，包内其余条目一律不
  落地。zip-slip（``../``）、符号链接逃逸因此在结构上不可能发生，而不是靠字符串过滤；
- zip 成员流式读取，不整块载入内存；目录源跳过符号链接；
- 写入先落 ``{models_root}/.importing/<model>``，校验齐备后才原子改名到目标目录，
  失败路径清理暂存，半截包不会污染可用的模型目录。

原子性口径：

- **包内命中但不完整的模型目录 → 整次导入失败**并列出缺失文件。半截目录（有权重、
  少 tokenizer）是打包方的问题，静默忽略会让「导入成功但模型仍不可用」变成谜题；
- 包内未出现的模型不导入；目标机已就绪的模型按幂等跳过（不覆盖可用文件）。
"""

from __future__ import annotations

import asyncio
import contextlib
import os
import shutil
import zipfile
from dataclasses import dataclass, replace
from pathlib import Path
from typing import TYPE_CHECKING

from app.core.logging import getLogger
from app.services.embedding_service import models_root
from app.services.model_download_service import model_ready
from app.services.model_specs import importable_models, resolve_spec, spec_ready

if TYPE_CHECKING:
    from app.services.model_specs import DownloadSpec

logger = getLogger("filemind.model_import")

#: 暂存目录名（位于 ``models_root`` 下的隐藏目录，导入成功才改名到模型目录）
_STAGING_DIR_NAME = ".importing"
#: zip 成员拷贝分块（8MB：顺序读写大文件，块大些可减少 syscall）
_COPY_CHUNK_BYTES = 8 * 1024 * 1024

#: 导入串行锁（并发导入会争用同一暂存目录；见 :func:`import_package`）
_import_lock = asyncio.Lock()


class PackageInvalidError(Exception):
    """离线包内容不符合要求（无可用模型 / 命中的模型目录不完整）。"""


class PackageSourceError(Exception):
    """离线包不可读（路径不存在 / zip 损坏 / 读取或落盘失败）。"""


@dataclass(frozen=True)
class ImportOutcome:
    """导入结果。

    Attributes:
        imported: 本次写入（或替换）的模型名。
        skipped: 包内已就绪、按幂等跳过的模型名。
    """

    imported: list[str]
    skipped: list[str]


@dataclass(frozen=True)
class _PackageSource:
    """离线包来源。

    目录源统一按**父目录**视角解析（``base`` = 父目录，``rel_paths`` 带上一级目录名），
    使「选中 models 目录」与「选中某个模型目录」两种选法落到同一套路径规则。
    """

    base: Path
    rel_paths: set[str]
    is_zip: bool

    def member_path(self, rel_path: str) -> Path:
        """目录源的成员绝对路径（zip 源不使用本方法）。"""
        return self.base / rel_path


@dataclass(frozen=True)
class _ModelPackage:
    """包内一个待导入的模型：来源 + 模型名 + 包内前缀。"""

    source: _PackageSource
    model: str
    spec: DownloadSpec
    prefix: str

    def copy_into(self, staging: Path) -> None:
        """按规格清单把本模型的文件取入暂存目录。

        Raises:
            OSError: 源文件缺失 / 落盘失败（调用方转 :class:`PackageSourceError`）。
            zipfile.BadZipFile: zip 结构损坏。
            KeyError: zip 内缺少该成员。
        """
        if self.source.is_zip:
            with zipfile.ZipFile(self.source.base) as archive:
                for name in self.spec.files:
                    self._copy_member(archive, name, _prepare(staging / name))
            return
        for name in self.spec.files:
            shutil.copyfile(
                self.source.member_path(f"{self.prefix}{self.model}/{name}"),
                _prepare(staging / name),
            )

    def _copy_member(self, archive: zipfile.ZipFile, name: str, dest: Path) -> None:
        """流式取出单个 zip 成员（不整块读入内存）。"""
        member = f"{self.prefix}{self.model}/{name}"
        with archive.open(member) as src, dest.open("wb") as out:
            shutil.copyfileobj(src, out, _COPY_CHUNK_BYTES)


def _prepare(dest: Path) -> Path:
    """建好目标文件的父目录并返回目标路径。"""
    dest.parent.mkdir(parents=True, exist_ok=True)
    return dest


def _rmtree(path: Path) -> None:
    """删除目录（不存在即 no-op）。"""
    with contextlib.suppress(OSError):
        shutil.rmtree(path)


def _is_zip(source: Path) -> bool:
    """是否按 zip 处理（按扩展名判定；非 zip 的普通文件随后被拒）。"""
    return source.is_file() and source.suffix.lower() == ".zip"


def _dir_rel_paths(root: Path) -> set[str]:
    """目录内全部文件的相对路径（posix；跳过符号链接，避免取到包外文件）。"""
    return {
        p.relative_to(root).as_posix()
        for p in root.rglob("*")
        if p.is_file() and not p.is_symlink()
    }


def _zip_rel_paths(source: Path) -> set[str]:
    """zip 内全部成员名（归一化为 posix，排除目录项）。

    Raises:
        PackageSourceError: zip 损坏 / 不可读。
    """
    try:
        with zipfile.ZipFile(source) as archive:
            return {i.filename.replace("\\", "/") for i in archive.infolist() if not i.is_dir()}
    except (zipfile.BadZipFile, OSError) as exc:
        raise PackageSourceError(f"zip 无法读取: {exc}") from exc


def _resolve_source(source: str) -> _PackageSource:
    """解析离线包来源（zip 文件或目录）。

    Raises:
        PackageSourceError: 路径不存在、既非 zip 也非目录、或 zip 不可读。
    """
    path = Path(source)
    if not path.exists():
        raise PackageSourceError(f"路径不存在: {source}")
    if _is_zip(path):
        return _PackageSource(path, _zip_rel_paths(path), True)
    if path.is_dir():
        # 父目录视角：rel_paths 带上一级目录名，两种选中方式共用同一套路径规则
        parent = path.parent
        return _PackageSource(
            parent,
            {f"{path.name}/{rel}" for rel in _dir_rel_paths(path)},
            False,
        )
    raise PackageSourceError("离线包须为 zip 文件或目录")


def _find_prefix(rel_paths: set[str], model: str, weight: str) -> str | None:
    """定位模型目录在包内的前缀（包根直放模型目录时为 ``""``）。"""
    suffix = f"{model}/{weight}"
    for rel in sorted(rel_paths):
        if rel == suffix:
            return ""
        if rel.endswith(f"/{suffix}"):
            return rel[: -(len(suffix) + 1)] + "/"
    return None


def _missing_files(
    rel_paths: set[str], model: str, prefix: str, files: tuple[str, ...]
) -> list[str]:
    """按规格清单列出包内缺失的文件（相对模型目录）。"""
    return [name for name in files if f"{prefix}{model}/{name}" not in rel_paths]


def _detect_packages(source: _PackageSource) -> list[_ModelPackage]:
    """探测包内可导入的模型。

    严入口径：命中权重但清单不全的模型目录会让整次导入失败——半截目录说明打包方
    漏了文件，静默跳过会留下「导入成功但模型仍不可用」的谜题。

    Raises:
        PackageInvalidError: 存在不完整的模型目录；或包内没有任何可导入的模型。
    """
    packages: list[_ModelPackage] = []
    incomplete: list[str] = []
    for model in importable_models():
        spec = resolve_spec(model)
        prefix = _find_prefix(source.rel_paths, model, spec.weight)
        if prefix is None:
            continue
        missing = _missing_files(source.rel_paths, model, prefix, spec.files)
        if missing:
            incomplete.append(f"{model} 缺少 {'、'.join(missing)}")
            continue
        packages.append(_ModelPackage(source, model, spec, prefix))
    if incomplete:
        raise PackageInvalidError(f"包内模型目录不完整（{'；'.join(incomplete)}）")
    if not packages:
        expected = "、".join(importable_models())
        raise PackageInvalidError(f"包内未找到模型目录（期望包内含 {expected} 之一）")
    return packages


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
        PackageInvalidError: 见 :func:`_detect_packages`。
        PackageSourceError: 见 :func:`_resolve_source` / :func:`_commit_package`。
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
