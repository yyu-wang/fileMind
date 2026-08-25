"""T4.3 — 元数据启发式分类层单元测试。

覆盖：文件名/扩展名/目录三策略命中、优先级短路、无命中返回 None、
大小写不敏感、中文目录名。
"""

from __future__ import annotations

import sys
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))  # noqa: E402

from app.rules.heuristic import HeuristicMatch, classify_heuristic  # noqa: E402
from app.rules.models import FileMeta  # noqa: E402


def make_file(
    name: str,
    parent: str = "/data",
    size: int = 1000,
) -> FileMeta:
    """构造测试文件（extension 自动取小写无点）。"""
    return FileMeta(
        name=name,
        extension=Path(name).suffix.lstrip(".").lower(),
        path=Path(parent) / name,
        size=size,
    )


def test_filename_pattern_wins_over_extension() -> None:
    """文件名关键词优先于扩展名：发票_2024.pdf → 财务。"""
    match = classify_heuristic(make_file("发票_2024.pdf"))
    assert match is not None
    assert match.category == "财务"
    assert match.method == "filename"


def test_filename_pattern_english_case_insensitive() -> None:
    """英文关键词大小写不敏感：INVOICE_2024.pdf → 财务。"""
    match = classify_heuristic(make_file("INVOICE_2024.pdf"))
    assert match is not None
    assert match.category == "财务"
    assert match.method == "filename"


def test_extension_mapping() -> None:
    """扩展名命中：texture_asset.svg → 图片。"""
    match = classify_heuristic(make_file("texture_asset.svg"))
    assert match is not None
    assert match.category == "图片"
    assert match.method == "extension"


def test_extension_mapping_docx() -> None:
    """docx → 办公文档。"""
    match = classify_heuristic(make_file("需求说明.docx"))
    assert match is not None
    assert match.category == "办公文档"
    assert match.method == "extension"


def test_directory_recognition_english() -> None:
    """目录命中：/Pictures/vacation 下未知扩展名 → 图片。"""
    file = make_file("notes.xyz", parent="/Users/me/Pictures/vacation")
    match = classify_heuristic(file)
    assert match is not None
    assert match.category == "图片"
    assert match.method == "directory"


def test_downloads_directory_no_longer_forced_archive() -> None:
    """SC-M7：/下载 不再硬判「压缩包」——杂项目录交扩展名/LLM 兜底。"""
    file = make_file("installer.xyz", parent="/Users/me/下载/temp")
    assert classify_heuristic(file) is None


def test_desktop_directory_no_longer_forced_document() -> None:
    """SC-M7：/桌面 不再硬判「文档」。"""
    file = make_file("weird.zzzz", parent="/Users/me/桌面")
    assert classify_heuristic(file) is None


def test_semantic_directories_still_recognized() -> None:
    """SC-M7 回归：语义成立的目录映射保留（archives/projects）。"""
    assert classify_heuristic(make_file("backup.xyz", parent="/data/archives")).category == "压缩包"
    assert (
        classify_heuristic(make_file("main.xyz", parent="/data/projects/demo")).category
        == "项目文件"
    )


def test_no_match_returns_none() -> None:
    """全部未命中 → None（交 LLM 层）。"""
    file = make_file("weird.zzzz", parent="/tmp")
    assert classify_heuristic(file) is None


def test_directory_loses_to_extension() -> None:
    """扩展名优先于目录：/Pictures 下的 png → 图片（method=extension）。"""
    file = make_file("family.png", parent="/Users/me/Pictures")
    match = classify_heuristic(file)
    assert match is not None
    assert match.method == "extension"


def test_filename_loses_to_nothing_and_wins() -> None:
    """文件名信号强于目录：/Pictures 下 会议纪要.xlsx → 文档（filename）。"""
    file = make_file("会议纪要.xlsx", parent="/Users/me/Pictures")
    match = classify_heuristic(file)
    assert match is not None
    assert match.category == "文档"
    assert match.method == "filename"


@pytest.mark.parametrize(
    ("name", "parent", "expected"),
    [
        ("发票_2024.pdf", "/data", "财务"),
        ("截图_20240815.png", "/data", "图片"),
        ("屏幕录制_会议.mp4", "/data", "视频"),
        ("会议录音.mp3", "/data", "音频"),
        ("首页设计稿.fig", "/data", "设计文件"),
        ("项目备份.zip", "/data", "压缩包"),
        ("2024周报.docx", "/data", "文档"),
        ("爬虫脚本.py", "/data", "代码"),
    ],
    ids=["发票", "截图", "录屏", "录音", "设计稿", "备份", "周报", "脚本"],
)
def test_filename_pattern_cases(
    name: str,
    parent: str,
    expected: str,
) -> None:
    """文件名关键词命中各类别。"""
    match = classify_heuristic(make_file(name, parent=parent))
    assert match is not None
    assert match.category == expected
    assert match.method == "filename"


def test_heuristic_match_fields() -> None:
    """命中结果为 HeuristicMatch，字段齐全。"""
    match = classify_heuristic(make_file("截图.png"))
    assert isinstance(match, HeuristicMatch)
    assert match.category and match.method
