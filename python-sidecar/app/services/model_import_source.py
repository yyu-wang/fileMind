"""离线模型包解析：来源识别（zip / 目录）、成员清单与包内模型探测。

（2026-09-18 按职责拆自 ``model_import_service.py``，333 行超 Python 警告阈值
300；暂存 → 校验 → 原子改名的落盘提交与对外编排仍在
:mod:`app.services.model_import_service`。）

包结构（就是 ``models`` 目录本身，或它的 zip）：:

    <模型目录名>/<规格声明的文件...>

例如 ``bge-large-zh-v1.5/onnx/model_quantized.onnx``、
``bge-reranker-v2-m3/model.safetensors``。也接受把模型目录套在外层目录里
（``models/bge-large-zh-v1.5/...``）；目录源还额外兼容「用户在文件选择器里直接
选中单个模型目录」——按**父目录**视角解析后，两种选法落到同一套路径规则上。

安全（对齐 ``rules/security.md``）：

- **白名单驱动取文件**：只按 ``resolve_spec`` 声明的清单逐个取，包内其余条目一律
  不落地。zip-slip（``../``）、符号链接逃逸因此在结构上不可能发生，而不是靠字符串
  过滤；
- zip 成员流式读取，不整块载入内存；目录源跳过符号链接。

错误口径（两类都由本模块定义，落盘提交侧复用）：

- :class:`PackageInvalidError`：包内没有可用模型，或命中的模型目录不完整；
- :class:`PackageSourceError`：包不可读（路径不存在 / zip 损坏 / 读取失败）。
"""

from __future__ import annotations

import shutil
import zipfile
from dataclasses import dataclass
from pathlib import Path
from typing import TYPE_CHECKING

from app.services.model_specs import importable_models, resolve_spec

if TYPE_CHECKING:
    from app.services.model_specs import DownloadSpec

#: zip 成员拷贝分块（8MB：顺序读写大文件，块大些可减少 syscall）
_COPY_CHUNK_BYTES = 8 * 1024 * 1024


class PackageInvalidError(Exception):
    """离线包内容不符合要求（无可用模型 / 命中的模型目录不完整）。"""


class PackageSourceError(Exception):
    """离线包不可读（路径不存在 / zip 损坏 / 读取或落盘失败）。"""


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
