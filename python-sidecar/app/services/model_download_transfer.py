"""T2 — 模型下载的 HTTP 传输层（原 ``model_download_service.py`` 拆出）。

只负责「把仓库里的单个文件搬到本地」这一层：SSL 上下文、远端大小探测、
流式下载（``.part`` → 原子改名）、进度节流与残留清理。镜像轮询、重试次数与
状态机在 :mod:`app.services.model_download_service`。

为什么不用 ``huggingface_hub`` 下载：其 xet 通道与 hf-mirror 不兼容、且 SDK 不给
逐字节进度。本模块直接用 httpx 流式 GET ``{mirror}/{repo}/resolve/main/{path}``，
镜像、重试、进度完全可控。

TLS 版本：**下载默认把 TLS 上限压到 1.2**。实测（M 系列 Mac / Python 3.12 /
OpenSSL 3.x）在 TLS 1.3 下访问 hf-mirror，无论 1KB 范围请求还是 10MB 分片，
都会在约 20s 后以 ``RemoteProtocolError`` / ``SSLV3_ALERT_BAD_RECORD_MAC``
失败；同一 URL 压到 TLS 1.2 后 1.6s 返回 206。证书校验保持开启，需要恢复
1.3 时设 ``FILEMIND_MODEL_TLS_MAX=1.3``。

进度口径：``total_bytes`` / ``downloaded_bytes`` **只统计 ONNX 权重文件**。
实测 hf-mirror 对辅助小文件（``tokenizer.json`` 等）既不返回 ``Content-Length``
也不支持 Range，无法得到可信总量；而权重占模型体积 99.99%（实测 312MB vs
辅助文件几十 KB），以它作为进度基准既准确又稳定。
"""

from __future__ import annotations

import contextlib
import os
import ssl
import time
from typing import TYPE_CHECKING

import httpx

from app.services.model_download_state import ModelDownloadError, _now_iso

if TYPE_CHECKING:
    from pathlib import Path

    from app.services.model_download_state import _State

#: TLS 上限环境变量：值为 "1.3" 时恢复默认协商，其余（含未设置）压到 TLS 1.2
_TLS_MAX_ENV = "FILEMIND_MODEL_TLS_MAX"
#: 每批读取的字节数（64KB，兼顾流式进度粒度与 syscall 次数）
_CHUNK_BYTES = 64 * 1024
#: 进度上报节流（秒）：避免每个 chunk 都改状态触发前端高频轮询抖动
PROGRESS_THROTTLE_SECONDS = float(os.environ.get("FILEMIND_MODEL_PROGRESS_THROTTLE", "0.5"))


def _ssl_context() -> ssl.SSLContext:
    """构造下载用 SSL 上下文（默认把上限压到 TLS 1.2，证书校验保持开启）。

    原因见模块 docstring：TLS 1.3 与 hf-mirror 的 CDN 组合下必然
    ``SSLV3_ALERT_BAD_RECORD_MAC``，压到 1.2 后正常。
    """
    context = ssl.create_default_context()
    if os.environ.get(_TLS_MAX_ENV, "1.2").strip() == "1.3":
        return context
    context.maximum_version = ssl.TLSVersion.TLSv1_2
    return context


async def _probe_size(client: httpx.AsyncClient, url: str) -> int | None:
    """探测远端文件字节数。

    首选 HEAD 的 ``Content-Length``；但 hf-mirror 对部分文件的 HEAD 响应不带该
    头部，此时用 ``Range: bytes=0-0`` 的 ``Content-Range`` 兜底。
    两者都拿不到 → ``None``（调用方按「进度不确定」处理）。
    """
    try:
        head_resp = await client.head(url)
        head_resp.raise_for_status()
        length = head_resp.headers.get("content-length")
        if length is not None:
            return int(length)
    except (httpx.HTTPError, ValueError):
        pass

    try:
        range_resp = await client.get(url, headers={"Range": "bytes=0-0"})
        range_resp.raise_for_status()
    except (httpx.HTTPError, ValueError):
        return None
    return _parse_content_range_total(range_resp.headers.get("content-range"))


def _parse_content_range_total(value: str | None) -> int | None:
    """解析 ``Content-Range: bytes 0-0/12345`` 的总长度；不可解析返回 ``None``。"""
    if value is None or "/" not in value:
        return None
    tail = value.rpartition("/")[2].strip()
    return int(tail) if tail.isdigit() else None


def _resolve_url(mirror: str, repo: str, path: str) -> str:
    """拼 resolve URL：``{mirror}/{repo}/resolve/main/{path}``。"""
    return f"{mirror}/{repo}/resolve/main/{path}"


async def _download_one(
    client: httpx.AsyncClient,
    mirror: str,
    repo: str,
    name: str,
    dest: Path,
    state: _State,
    *,
    count_progress: bool,
) -> None:
    """流式下载单个文件：先写 ``.part``，完成后原子改名。

    Args:
        client: 当前尝试使用的 httpx 客户端。
        mirror: 当前镜像地址。
        repo: HuggingFace 仓库 id（来自下载规格）。
        name: 仓库内相对路径。
        dest: 落盘目标路径。
        state: 对应模型的状态对象（进度写入处）。
        count_progress: 是否把本文件字节计入 ``downloaded_bytes``（仅权重文件为 True，
            与 ``total_bytes`` 的口径一致）。

    Raises:
        httpx.HTTPError: 网络/状态码失败（由 ``_download_all`` 捕获切换镜像）。
        OSError: 落盘失败。
        ModelDownloadError: 下载内容为空。
    """
    dest.parent.mkdir(parents=True, exist_ok=True)
    part = dest.with_name(f"{dest.name}.part")
    async with client.stream("GET", _resolve_url(mirror, repo, name)) as resp:
        resp.raise_for_status()
        written = 0
        with part.open("wb") as fh:
            async for chunk in resp.aiter_bytes(_CHUNK_BYTES):
                fh.write(chunk)
                written += len(chunk)
                if count_progress:
                    state.downloaded_bytes += len(chunk)
                    _report_progress(state)
    if written == 0:
        raise ModelDownloadError(f"下载内容为空: {name}")
    os.replace(part, dest)
    state.updated_at = _now_iso()


def _report_progress(state: _State) -> None:
    """按节流间隔刷新 ``updated_at``（进度字节数实时累加，仅时间戳节流）。"""
    now = time.monotonic()
    if now - state.last_report >= PROGRESS_THROTTLE_SECONDS:
        state.last_report = now
        state.updated_at = _now_iso()


def _cleanup_partials(root: Path, targets: tuple[str, ...]) -> None:
    """清理本轮残留的 ``.part`` 文件（失败重试前调用，避免脏数据占空间）。"""
    for name in targets:
        part = (root / name).with_name(f"{(root / name).name}.part")
        with contextlib.suppress(OSError):
            part.unlink(missing_ok=True)
