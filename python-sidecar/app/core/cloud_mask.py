"""云端模式 LLM payload 数据脱敏（安全 07-I-01 / T7.2）。

云端推理激活时，任何发往云端 LLM 的 payload 都必须在提示词构建出口经过本模块
脱敏：真实文件名编号化为 ``file_001``（保留扩展名供 LLM 判型）、完整路径收敛为
目录深度（``depth=N``，绝不发送相对路径——相对路径仍含目录名）、内容截断为
固定上限；并维护「编号 ↔ 真实文件」的可还原映射（DoD：云端请求 payload 中
不含真实文件名/路径）。

门控默认关闭（``FILEMIND_CLOUD_MASKING`` 未置 1）——当前云端模式不可达
（T7.4 代理转发 / T7.5 知情同意激活），本地 Ollama 直连路径行为零变化。
T7.4 激活云端模式时置门控为 1，提示词出口自动生效。
"""

from __future__ import annotations

import os
import re
from dataclasses import dataclass

from app.core.logging import getLogger

logger = getLogger("filemind.cloud_mask")

#: 单文件内容截断默认上限（字符）。对齐 P-01 content_summary 与 P-03 CONTENT_MAX=500
DEFAULT_CONTENT_MAX = 500
#: 环境变量：云端脱敏总开关（值 "1" 开启）
_ENV_MASKING = "FILEMIND_CLOUD_MASKING"
#: 环境变量：内容截断上限覆盖（正整数，非法值回落默认）
_ENV_CONTENT_MAX = "FILEMIND_CLOUD_CONTENT_MAX"

#: 扩展名白名单（仅字母数字，1-10 位）：保留类型信号，且不含路径注入字符
_EXT_RE = re.compile(r"\.[A-Za-z0-9]{1,10}$")


@dataclass(frozen=True)
class MaskedFile:
    """单文件脱敏结果 + 还原所需信息。"""

    key: str
    """调用方侧唯一标识（P-01 用真实路径，P-03 用 file_name）。"""

    seq: int
    """批内序号（1 起始）。"""

    mask_name: str
    """脱敏文件名（``file_001.txt``）。"""

    real_name: str
    """原始文件名（还原用）。"""

    real_path: str | None
    """原始完整路径（还原用；P-03 片段无路径时为 None）。"""

    depth: int
    """目录深度（路径脱敏结果）。"""

    content_head: str
    """截断后的内容摘要（供 LLM 使用）。"""


def mask_filename(name: str, seq: int) -> str:
    """真实文件名 → ``file_{seq:03d}{ext}``。

    扩展名保留小写（如 ``report.PDF`` → ``file_001.pdf``）供 LLM 判型；
    无扩展名或扩展名含非字母数字时省略。序号 1 起始、3 位补零。
    """
    match = _EXT_RE.search(name)
    suffix = match.group(0).lower() if match else ""
    return f"file_{seq:03d}{suffix}"


def mask_path_to_depth(path: str) -> int:
    """完整路径 → 目录深度（非空段数）。POSIX/Windows 分隔符归一化后计数。

    例：``/a/b/c.txt`` → 3；``C:\\a\\b\\c.txt`` → 4（含盘符段）；``/`` → 0。
    """
    normalized = path.replace("\\", "/")
    return sum(1 for seg in normalized.split("/") if seg)


def truncate_content(text: str, limit: int = DEFAULT_CONTENT_MAX) -> str:
    """内容截断为前 ``limit`` 字符（字符级切片，不切断 astral 字符）。"""
    return text[:limit]


class CloudMasker:
    """云端 payload 脱敏器：编号化 + 深度化 + 截断，并登记可还原映射。

    同一调用内同一文件必须传同一 ``key``；``mask_file`` 对同 key 幂等
    （重复脱敏返回相同编号），保证 LLM 引用的 ``file_003`` 与真实文件一一对应。
    映射生命周期 = 本次调用：云端结果返回后由 ``resolve`` 还原到真实文件。
    """

    def __init__(self, content_limit: int = DEFAULT_CONTENT_MAX) -> None:
        self._content_limit = content_limit
        self._by_key: dict[str, MaskedFile] = {}
        self._next_seq = 1

    def mask_file(
        self,
        key: str,
        name: str,
        path: str | None,
        content: str,
    ) -> MaskedFile:
        """登记并返回脱敏结果；同 key 幂等（编号不重复分配）。"""
        existing = self._by_key.get(key)
        if existing is not None:
            return existing
        seq = self._next_seq
        self._next_seq += 1
        masked = MaskedFile(
            key=key,
            seq=seq,
            mask_name=mask_filename(name, seq),
            real_name=name,
            real_path=path,
            depth=mask_path_to_depth(path) if path else 0,
            content_head=truncate_content(content, self._content_limit),
        )
        self._by_key[key] = masked
        return masked

    def resolve(self, key: str) -> MaskedFile | None:
        """DoD 还原：按 key 取回真实文件名/路径；未知 key 返回 None。"""
        return self._by_key.get(key)

    def stats(self) -> dict[str, int]:
        """审计用量：文件数与发送内容总字符数（不含任何内容/文件名）。"""
        return {
            "files": len(self._by_key),
            "chars": sum(len(m.content_head) for m in self._by_key.values()),
        }


def is_cloud_masking_active() -> bool:
    """云端脱敏门控：``FILEMIND_CLOUD_MASKING=1`` 开启，默认关闭。"""
    return os.environ.get(_ENV_MASKING, "0") == "1"


def content_max() -> int:
    """内容截断上限：默认 500，env ``FILEMIND_CLOUD_CONTENT_MAX`` 可覆盖。

    非法值（非整数 / 非正数）回落默认，避免配置错误导致截断失效。
    """
    raw = os.environ.get(_ENV_CONTENT_MAX, "")
    try:
        value = int(raw)
    except ValueError:
        return DEFAULT_CONTENT_MAX
    return value if value > 0 else DEFAULT_CONTENT_MAX


def log_cloud_call(provider: str, masker: CloudMasker) -> None:
    """云端调用审计：记录 Provider 与发送数据量（文件数/字符数），不含内容与文件名。

    对齐 07-I-01「每次云端调用记录发送的数据量和目标 Provider（不含内容本身）」；
    由云端传输层（T7.4）在实际转发时调用。
    """
    stats = masker.stats()
    logger.info(
        "cloud.call",
        provider=provider,
        files=stats["files"],
        chars=stats["chars"],
    )
