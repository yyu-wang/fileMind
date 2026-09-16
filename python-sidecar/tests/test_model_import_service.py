"""T1 — 离线模型包导入服务单元测试。

覆盖：目录源（models 目录 / 直接选中单个模型目录）、zip 源（含外层目录前缀）、
多模型包、已就绪幂等跳过、替换不完整的旧目录、以及全部失败路径
（包内无模型 / 模型目录缺文件 / 非 zip 普通文件 / 路径不存在 / zip 损坏）。

安全回归：包内 ``../`` 路径、绝对路径与符号链接条目一律不落地——取文件是
**白名单驱动**（只按 ``resolve_spec`` 清单逐个取），而非解压全包后过滤。

模型目录用 ``FILEMIND_MODEL_DIR`` 指向 tmp_path 隔离，不发真实网络请求。
"""

from __future__ import annotations

import sys
import zipfile
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))  # noqa: E402

from app.services.model_download_service import model_ready  # noqa: E402
from app.services.model_import_service import (  # noqa: E402
    PackageInvalidError,
    PackageSourceError,
    import_package,
)
from app.services.model_specs import (  # noqa: E402
    LLM_MODEL_NAME,
    RERANK_MODEL_NAME,
    importable_models,
    resolve_spec,
)

EMBEDDING = "bge-large-zh-v1.5"
#: 与真实权重无关的占位字节数（就绪判定只要求存在且非空）
_FAKE_BYTES = b"\x00" * 32


@pytest.fixture
def models_root(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> Path:
    """把本机模型目录指向 tmp_path/target（隔离真实 ~/.filemind）。"""
    target = tmp_path / "target"
    monkeypatch.setenv("FILEMIND_MODEL_DIR", str(target))
    return target


def _make_model_dir(root: Path, model: str, *, skip: str | None = None) -> Path:
    """在 root 下造一个模型目录（``skip`` 指定省略的文件，用于构造不完整包）。"""
    target = root / model
    for name in resolve_spec(model).files:
        if name == skip:
            continue
        path = target / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(_FAKE_BYTES)
    return target


def _zip_dir(zip_path: Path, source_dir: Path, *, arc_prefix: str = "") -> None:
    """把目录打成 zip（``arc_prefix`` 控制包内是否多套一层目录）。"""
    with zipfile.ZipFile(zip_path, "w") as archive:
        for path in source_dir.rglob("*"):
            if path.is_file():
                archive.write(path, f"{arc_prefix}{path.relative_to(source_dir).as_posix()}")


# ------------------------------------------------------------------
# 成功路径
# ------------------------------------------------------------------


async def test_import_from_models_dir(models_root: Path, tmp_path: Path) -> None:
    """包 = models 目录（外层多一层）→ 模型落入本机目录。"""
    package = tmp_path / "pkg" / "models"
    _make_model_dir(package, EMBEDDING)

    outcome = await import_package(str(package))

    assert outcome.imported == [EMBEDDING]
    assert outcome.skipped == []
    assert model_ready(EMBEDDING)
    assert not (models_root / ".importing").exists()


async def test_import_from_single_model_dir(models_root: Path, tmp_path: Path) -> None:
    """包 = 直接选中的单个模型目录（父目录视角解析）→ 同样可导入。"""
    package_parent = tmp_path / "pkg"
    package = _make_model_dir(package_parent, EMBEDDING)

    outcome = await import_package(str(package))

    assert outcome.imported == [EMBEDDING]
    assert model_ready(EMBEDDING)


async def test_import_from_zip_with_nested_prefix(models_root: Path, tmp_path: Path) -> None:
    """包 = zip（含 ``filemind-models/models/`` 两层前缀）→ 前缀自动定位。"""
    staging = tmp_path / "staging" / "models"
    _make_model_dir(staging, EMBEDDING)
    zip_path = tmp_path / "offline.zip"
    _zip_dir(zip_path, tmp_path / "staging", arc_prefix="filemind-models/")

    outcome = await import_package(str(zip_path))

    assert outcome.imported == [EMBEDDING]
    assert model_ready(EMBEDDING)


async def test_import_both_models_from_zip(models_root: Path, tmp_path: Path) -> None:
    """包内含 Embedding + Rerank → 一次性全部导入。"""
    staging = tmp_path / "staging"
    _make_model_dir(staging, EMBEDDING)
    _make_model_dir(staging, RERANK_MODEL_NAME)
    zip_path = tmp_path / "offline.zip"
    _zip_dir(zip_path, staging)

    outcome = await import_package(str(zip_path))

    assert sorted(outcome.imported) == sorted([EMBEDDING, RERANK_MODEL_NAME])
    assert model_ready(EMBEDDING)
    assert model_ready(RERANK_MODEL_NAME)


async def test_ready_model_is_skipped(models_root: Path, tmp_path: Path) -> None:
    """本机已就绪 → 幂等跳过，不覆盖可用文件。"""
    _make_model_dir(models_root, EMBEDDING)
    package = tmp_path / "pkg" / "models"
    _make_model_dir(package, EMBEDDING)

    outcome = await import_package(str(package))

    assert outcome.imported == []
    assert outcome.skipped == [EMBEDDING]


async def test_import_gguf_generation_model(models_root: Path, tmp_path: Path) -> None:
    """GGUF 生成模型走同一条离线通道（T2：内网分发不限于 embedding/rerank）。

    GGUF 是单文件权重、无辅助文件，是「包结构只有一层文件」的边界形态。
    """
    package = tmp_path / "pkg" / "models"
    _make_model_dir(package, LLM_MODEL_NAME)

    outcome = await import_package(str(package))

    assert outcome.imported == [LLM_MODEL_NAME]
    assert model_ready(LLM_MODEL_NAME)


async def test_replaces_incomplete_existing_dir(models_root: Path, tmp_path: Path) -> None:
    """本机残留半截目录（缺 tokenizer）→ 被完整包替换。"""
    _make_model_dir(models_root, EMBEDDING, skip="tokenizer.json")
    assert not model_ready(EMBEDDING)
    package = tmp_path / "pkg" / "models"
    _make_model_dir(package, EMBEDDING)

    outcome = await import_package(str(package))

    assert outcome.imported == [EMBEDDING]
    assert model_ready(EMBEDDING)
    assert not (models_root / ".importing").exists()


# ------------------------------------------------------------------
# 安全：白名单驱动取文件
# ------------------------------------------------------------------


async def test_zip_slip_and_symlink_entries_never_landed(models_root: Path, tmp_path: Path) -> None:
    """包内 ``../`` / 绝对路径 / 符号链接条目不落地，也不影响正常模型导入。"""
    models = tmp_path / "pkg" / "models"
    _make_model_dir(models, EMBEDDING)
    evil_zip = tmp_path / "evil.zip"
    with zipfile.ZipFile(evil_zip, "w") as archive:
        for path in models.rglob("*"):
            if path.is_file():
                archive.write(path, f"models/{path.relative_to(models).as_posix()}")
        # 路径穿越与绝对路径条目
        archive.writestr("../../escaped.txt", "pwned")
        archive.writestr("/escaped-abs.txt", "pwned")
        # 符号链接条目（external_attr 高 16 位 = 0o120777）
        link = zipfile.ZipInfo(f"models/{EMBEDDING}/symlink")
        link.external_attr = 0o120777 << 16
        archive.writestr(link, "/etc/passwd")

    outcome = await import_package(str(evil_zip))

    assert outcome.imported == [EMBEDDING]
    # 穿越目标不落地：包根上一级 / 系统根都干净
    assert not (tmp_path / "pkg" / "escaped.txt").exists()
    assert not (tmp_path / "escaped.txt").exists()
    # 非清单条目不落地
    assert not (models_root / EMBEDDING / "symlink").exists()


# ------------------------------------------------------------------
# 失败路径
# ------------------------------------------------------------------


async def test_incomplete_model_dir_rejected(models_root: Path, tmp_path: Path) -> None:
    """命中权重但清单不全 → 整次失败并点名缺失文件，且不落半截模型目录。"""
    package = tmp_path / "pkg" / "models"
    _make_model_dir(package, EMBEDDING, skip="tokenizer.json")

    with pytest.raises(PackageInvalidError, match="不完整"):
        await import_package(str(package))

    assert not model_ready(EMBEDDING)
    with pytest.raises(PackageInvalidError, match="tokenizer.json"):
        await import_package(str(package))


async def test_package_without_model_rejected(models_root: Path, tmp_path: Path) -> None:
    """包内没有任何模型目录 → EMB-V-001 语义的错误。"""
    package = tmp_path / "pkg"
    package.mkdir()
    (package / "readme.txt").write_text("只有说明文件", encoding="utf-8")

    with pytest.raises(PackageInvalidError, match="未找到模型目录"):
        await import_package(str(package))


async def test_missing_path_rejected(models_root: Path, tmp_path: Path) -> None:
    """路径不存在 → PackageSourceError。"""
    with pytest.raises(PackageSourceError, match="路径不存在"):
        await import_package(str(tmp_path / "nope.zip"))


async def test_plain_file_rejected(models_root: Path, tmp_path: Path) -> None:
    """非 zip 的普通文件 → PackageSourceError（避免把任意文件当包读）。"""
    plain = tmp_path / "pkg.txt"
    plain.write_text("not a package", encoding="utf-8")

    with pytest.raises(PackageSourceError, match="zip 文件或目录"):
        await import_package(str(plain))


async def test_broken_zip_rejected(models_root: Path, tmp_path: Path) -> None:
    """zip 损坏 → PackageSourceError（不冒泡 zipfile 异常）。"""
    broken = tmp_path / "broken.zip"
    broken.write_bytes(b"not a zip at all")

    with pytest.raises(PackageSourceError, match="zip 无法读取"):
        await import_package(str(broken))


async def test_target_occupied_by_file_rejected(models_root: Path, tmp_path: Path) -> None:
    """目标位置被同名文件占用 → 包装为 PackageSourceError（而非未包装的裸 IO 异常）。

    回归：路由只认 PackageSourceError；裸 OSError 冒泡会退化成没有错误码的 500，
    用户看不到「模型导入失败」与原因。
    """
    models_root.mkdir(parents=True, exist_ok=True)
    (models_root / EMBEDDING).write_text("占位文件", encoding="utf-8")
    package = tmp_path / "pkg" / "models"
    _make_model_dir(package, EMBEDDING)

    with pytest.raises(PackageSourceError, match="写入目标目录失败"):
        await import_package(str(package))


def test_importable_models_cover_both_sources() -> None:
    """可导入清单 = Embedding 注册表 + Rerank（新增模型时两处提示语的唯一来源）。"""
    names = importable_models()
    assert EMBEDDING in names
    assert RERANK_MODEL_NAME in names
