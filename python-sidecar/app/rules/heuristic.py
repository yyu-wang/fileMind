"""元数据启发式分类层：文件名模式 + 扩展名映射 + 目录结构识别。

来源：08_Prompt工程设计 §7 三层分类漏斗第 2 层（约 20% 文件），规则层未命中时调用。
三个信号按「文件名模式 → 扩展名映射 → 目录识别」优先级短路，全部未命中返回 None（交 LLM 层）。
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import TYPE_CHECKING, Literal

if TYPE_CHECKING:
    from app.rules.models import FileMeta


@dataclass(frozen=True)
class HeuristicMatch:
    """启发式命中结果，``method`` 标识命中的子策略。"""

    category: str
    method: Literal["filename", "extension", "directory"]


#: 文件名关键词 → 分类（有序，特异词在前，大小写不敏感子串匹配）
FILENAME_PATTERNS: tuple[tuple[str, str], ...] = (
    # 财务
    ("发票", "财务"),
    ("invoice", "财务"),
    ("收据", "财务"),
    ("报销", "财务"),
    ("财报", "财务"),
    # 合同
    ("合同", "合同"),
    ("contract", "合同"),
    # 简历
    ("简历", "简历"),
    ("resume", "简历"),
    # 图片
    ("截图", "图片"),
    ("screenshot", "图片"),
    ("screen shot", "图片"),
    ("照片", "图片"),
    ("photo", "图片"),
    ("头像", "图片"),
    ("壁纸", "图片"),
    # 视频
    ("录屏", "视频"),
    ("屏幕录制", "视频"),
    ("vlog", "视频"),
    ("宣传片", "视频"),
    # 音频
    ("录音", "音频"),
    ("播客", "音频"),
    ("podcast", "音频"),
    # 设计
    ("设计稿", "设计文件"),
    ("logo", "设计文件"),
    ("mockup", "设计文件"),
    ("原型图", "设计文件"),
    ("海报", "设计文件"),
    # 压缩包
    ("备份", "压缩包"),
    ("backup", "压缩包"),
    ("安装包", "压缩包"),
    ("归档", "压缩包"),
    # 文档
    ("会议纪要", "文档"),
    ("会议记录", "文档"),
    ("年度总结", "文档"),
    ("周报", "文档"),
    ("读书笔记", "文档"),
    ("报告", "文档"),
    ("report", "文档"),
    # 代码
    ("源码", "代码"),
    ("脚本", "代码"),
    ("leetcode", "代码"),
)

#: 扩展名 → 分类（对齐 Rust HEURISTIC_EXT_MAP + gen_testdata 类别规格）
EXTENSION_MAP: dict[str, str] = {
    # 图片
    "png": "图片",
    "jpg": "图片",
    "jpeg": "图片",
    "gif": "图片",
    "webp": "图片",
    "svg": "图片",
    "bmp": "图片",
    "ico": "图片",
    "avif": "图片",
    "heic": "图片",
    "heif": "图片",
    "tiff": "图片",
    # 文档
    "pdf": "文档",
    "txt": "文档",
    "md": "文档",
    "rtf": "文档",
    "odt": "文档",
    "epub": "文档",
    "mobi": "文档",
    "pages": "文档",
    # 办公文档 / 表格
    "doc": "办公文档",
    "docx": "办公文档",
    "ppt": "办公文档",
    "pptx": "办公文档",
    "odp": "办公文档",
    "xls": "表格",
    "xlsx": "表格",
    "ods": "表格",
    "csv": "表格",
    # 视频
    "mp4": "视频",
    "mkv": "视频",
    "mov": "视频",
    "avi": "视频",
    "wmv": "视频",
    "flv": "视频",
    "webm": "视频",
    "m4v": "视频",
    # 音频
    "mp3": "音频",
    "wav": "音频",
    "flac": "音频",
    "aac": "音频",
    "ogg": "音频",
    "m4a": "音频",
    # 代码
    "py": "代码",
    "js": "代码",
    "ts": "代码",
    "rs": "代码",
    "go": "代码",
    "java": "代码",
    "cpp": "代码",
    "c": "代码",
    "h": "代码",
    "rb": "代码",
    "php": "代码",
    "swift": "代码",
    "kt": "代码",
    "html": "代码",
    "css": "代码",
    "sh": "代码",
    # 压缩包
    "zip": "压缩包",
    "rar": "压缩包",
    "7z": "压缩包",
    "tar": "压缩包",
    "gz": "压缩包",
    "bz2": "压缩包",
    "xz": "压缩包",
    # 设计文件
    "psd": "设计文件",
    "ai": "设计文件",
    "skp": "设计文件",
    "fig": "设计文件",
    "xd": "设计文件",
    "sketch": "设计文件",
    # 数据文件
    "json": "数据文件",
    "xml": "数据文件",
    "yaml": "数据文件",
    "yml": "数据文件",
    "sql": "数据文件",
    "db": "数据文件",
    "sqlite": "数据文件",
    "log": "数据文件",
    "dat": "数据文件",
    "bin": "数据文件",
}

#: 目录组件名 → 分类（祖先目录精确匹配，大小写不敏感）
DIRECTORY_PATTERNS: tuple[tuple[str, str], ...] = (
    ("pictures", "图片"),
    ("photos", "图片"),
    ("photo", "图片"),
    ("images", "图片"),
    ("image", "图片"),
    ("图片", "图片"),
    ("documents", "文档"),
    ("docs", "文档"),
    ("文稿", "文档"),
    ("desktop", "文档"),
    ("桌面", "文档"),
    ("文档", "文档"),
    ("videos", "视频"),
    ("video", "视频"),
    ("movies", "视频"),
    ("movie", "视频"),
    ("视频", "视频"),
    ("music", "音频"),
    ("audio", "音频"),
    ("音乐", "音频"),
    ("音频", "音频"),
    ("downloads", "压缩包"),
    ("下载", "压缩包"),
    ("archives", "压缩包"),
    ("archive", "压缩包"),
    ("压缩包", "压缩包"),
    ("code", "代码"),
    ("source", "代码"),
    ("代码", "代码"),
    ("projects", "项目文件"),
    ("workspace", "项目文件"),
    ("项目", "项目文件"),
    ("工作空间", "项目文件"),
)


def classify_heuristic(file: FileMeta) -> HeuristicMatch | None:
    """对文件执行启发式分类，返回首个命中的结果；无命中返回 None。

    Args:
        file: 待分类文件元数据。

    Returns:
        命中结果（含分类与命中方式）；规则层未覆盖时返回 None 交给 LLM 兜底。
    """
    name_lower = file.name.lower()
    for keyword, category in FILENAME_PATTERNS:
        if keyword in name_lower:
            return HeuristicMatch(category=category, method="filename")

    if file.extension in EXTENSION_MAP:
        return HeuristicMatch(category=EXTENSION_MAP[file.extension], method="extension")

    for parent in file.path.parents:
        component = parent.name.lower()
        for keyword, category in DIRECTORY_PATTERNS:
            if keyword == component:
                return HeuristicMatch(category=category, method="directory")

    return None
