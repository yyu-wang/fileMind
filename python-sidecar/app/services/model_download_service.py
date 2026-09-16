"""T2 — 模型下载服务（HF 镜像轮询 + 自动重试 + 进度上报）。

职责：把「下载规格」（[`resolve_spec`]）声明的权重与 tokenizer/配置文件下载到本地
模型目录，并向设置页暴露可查询的下载状态（进度、当前镜像、尝试次数、失败原因）。

两类模型共用本模块的下载管线，仅来源与落盘位置不同：

- **Embedding**：来自注册表 ``embedding_models``（``hf_repo`` + ``onnx_file``），
  落盘 ``{models_root}/{model}/{onnx_file}``；
- **Rerank**：``BAAI/bge-reranker-v2-m3``（sentence-transformers cross-encoder）。
  它不在 Embedding 注册表里（无 dim / 表名语义），落盘 ``{models_root}/{model}/``。
  这是「装到别的机器也能用全部功能」的关键一环：全新机器没有 HF 缓存、镜像又可能
  不可达，而 ``rerank_service`` 会优先加载这里的本地副本（见 ``resolve_load_target``）。

为什么不用 ``huggingface_hub`` 下载：其 xet 通道与 hf-mirror 不兼容、且 SDK 不给
逐字节进度。本模块直接用 httpx 流式 GET ``{mirror}/{repo}/resolve/main/{path}``，
镜像、重试、进度完全可控。

TLS 版本：**下载默认把 TLS 上限压到 1.2**。实测（M 系列 Mac / Python 3.12 /
OpenSSL 3.x）在 TLS 1.3 下访问 hf-mirror，无论 1KB 范围请求还是 10MB 分片，
都会在约 20s 后以 ``RemoteProtocolError`` / ``SSLV3_ALERT_BAD_RECORD_MAC``
失败；同一 URL 压到 TLS 1.2 后 1.6s 返回 206。证书校验保持开启，需要恢复
1.3 时设 ``FILEMIND_MODEL_TLS_MAX=1.3``。

镜像与重试策略（需求约定）：
- 镜像序列默认 ``hf-mirror.com`` → ``huggingface.co``（env 可覆盖）
- 自动尝试最多 :data:`MAX_ATTEMPTS`（默认 3）次，每次失败切换到下一个镜像
- 用尽后停止自动重试，状态置 ``failed``，等待用户在设置页手动点击重试
  （手动重试会重新计数）

状态语义（``status``）：``idle`` 未开始 / ``downloading`` 进行中 /
``ready`` 文件齐备 / ``failed`` 自动重试用尽。所有文件先写 ``.part`` 再原子
改名，避免半截文件被判定为就绪。

进度口径：``total_bytes`` / ``downloaded_bytes`` **只统计 ONNX 权重文件**。
实测 hf-mirror 对辅助小文件（``tokenizer.json`` 等）既不返回 ``Content-Length``
也不支持 Range，无法得到可信总量；而权重占模型体积 99.99%（实测 312MB vs
辅助文件几十 KB），以它作为进度基准既准确又稳定。
"""

from __future__ import annotations

import asyncio
import contextlib
import os
import ssl
import time
from dataclasses import dataclass, field
from typing import TYPE_CHECKING

import httpx

from app.core.logging import getLogger
from app.models import ModelDownloadStatusResponse
from app.services.embedding_service import EmbeddingUnavailableError
from app.services.model_specs import resolve_spec, spec_ready

if TYPE_CHECKING:
    from pathlib import Path

    from app.services.model_specs import DownloadSpec

logger = getLogger("filemind.model_download")

#: 镜像序列（按顺序尝试；env 用逗号分隔覆盖）
MIRRORS: tuple[str, ...] = tuple(
    part.strip().rstrip("/")
    for part in os.environ.get(
        "FILEMIND_MODEL_MIRRORS", "https://hf-mirror.com,https://huggingface.co"
    ).split(",")
    if part.strip()
)
#: 自动尝试总次数（用尽后置 failed，等用户手动重试）
MAX_ATTEMPTS = int(os.environ.get("FILEMIND_MODEL_DOWNLOAD_ATTEMPTS", "3"))
#: 单个文件的下载超时（秒）；权重 300MB+，给足预算
DOWNLOAD_TIMEOUT = float(os.environ.get("FILEMIND_MODEL_DOWNLOAD_TIMEOUT", "600"))
#: 进度上报节流（秒）：避免每个 chunk 都改状态触发前端高频轮询抖动
PROGRESS_THROTTLE_SECONDS = float(os.environ.get("FILEMIND_MODEL_PROGRESS_THROTTLE", "0.5"))
#: TLS 上限环境变量：值为 "1.3" 时恢复默认协商，其余（含未设置）压到 TLS 1.2
_TLS_MAX_ENV = "FILEMIND_MODEL_TLS_MAX"
#: 每批读取的字节数（64KB，兼顾流式进度粒度与 syscall 次数）
_CHUNK_BYTES = 64 * 1024

_STATUS_IDLE = "idle"
_STATUS_DOWNLOADING = "downloading"
_STATUS_READY = "ready"
_STATUS_FAILED = "failed"


class ModelDownloadError(Exception):
    """下载内容异常（内容为空等）：由 :func:`_download_all` 捕获后切换镜像重试。"""


@dataclass
class _State:
    """单个模型的下载状态（进程内，可变）。"""

    model_name: str
    status: str = _STATUS_IDLE
    mirror: str | None = None
    attempt: int = 0
    downloaded_bytes: int = 0
    total_bytes: int | None = None
    error: str | None = None
    updated_at: str = ""
    #: 进度节流用的最近一次上报时间（monotonic），不对外暴露
    last_report: float = field(default=0.0, repr=False)

    def as_response(self) -> ModelDownloadStatusResponse:
        """转换为对外响应模型。"""
        return ModelDownloadStatusResponse(
            model_name=self.model_name,
            status=self.status,
            mirror=self.mirror,
            attempt=self.attempt,
            downloaded_bytes=self.downloaded_bytes,
            total_bytes=self.total_bytes,
            error=self.error,
            updated_at=self.updated_at,
        )


#: 每模型状态（model_name → _State）
_states: dict[str, _State] = {}
#: 进行中的下载任务（model_name → Task），用于幂等与避免重复启动
_tasks: dict[str, asyncio.Task[None]] = {}
#: 状态字典的并发保护（启动任务 / 读状态）
_lock = asyncio.Lock()


def reset_state() -> None:
    """清空状态与任务记录（仅测试用）。"""
    _states.clear()
    _tasks.clear()


def files_for(model: str) -> tuple[str, ...]:
    """该模型需要下载的仓库文件（权重 + tokenizer/配置）。

    Raises:
        ValueError: 模型不在任一来源中（由调用方转 HTTP 400）。
    """
    return resolve_spec(model).files


def weight_file(model: str) -> str:
    """权重文件的仓库内路径（进度统计基准）。

    Raises:
        ValueError: 模型不在任一来源中。
    """
    return resolve_spec(model).weight


def model_ready(model: str) -> bool:
    """本地文件是否齐备（所有目标文件存在且非空）；未知模型返回 ``False``。"""
    try:
        spec = resolve_spec(model)
    except (EmbeddingUnavailableError, ValueError):
        return False
    return spec_ready(spec)


def _now_iso() -> str:
    """当前时间的 ISO 8601 字符串（秒精度）。"""
    return time.strftime("%Y-%m-%dT%H:%M:%S", time.localtime())


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


def _state_for(model: str) -> _State:
    """取（或新建）某模型的状态；本地已就绪时直接标记 ready。"""
    state = _states.get(model)
    if state is None:
        state = _State(model_name=model)
        if model_ready(model):
            state.status = _STATUS_READY
        state.updated_at = _now_iso()
        _states[model] = state
    return state


def get_status(model: str) -> ModelDownloadStatusResponse:
    """查询下载状态（未开始且本地已就绪时返回 ready）。

    Raises:
        ValueError: 模型不在任一来源中。
    """
    resolve_spec(model)  # 未知模型 → ValueError
    return _state_for(model).as_response()


async def ensure_downloaded(model: str) -> ModelDownloadStatusResponse:
    """确保模型可用：已就绪直接返回，否则启动后台下载并立即返回当前状态。

    幂等：已有进行中的任务时不重复启动。``failed`` 状态下再次调用表示用户
    手动重试，会重新计数。

    Raises:
        ValueError: 模型不在任一来源中。
    """
    resolve_spec(model)
    async with _lock:
        state = _state_for(model)
        if state.status == _STATUS_READY:
            return state.as_response()
        task = _tasks.get(model)
        if task is not None and not task.done():
            return state.as_response()
        state.status = _STATUS_DOWNLOADING
        state.attempt = 0
        state.downloaded_bytes = 0
        state.error = None
        state.updated_at = _now_iso()
        _tasks[model] = asyncio.create_task(_download_all(model))
        return state.as_response()


def _start_attempt(state: _State, model: str, attempt: int) -> str:
    """开始一轮下载尝试：记录镜像/次数并重置进度，返回本轮镜像地址。"""
    mirror = MIRRORS[(attempt - 1) % len(MIRRORS)]
    state.attempt = attempt
    state.mirror = mirror
    state.downloaded_bytes = 0
    state.updated_at = _now_iso()
    logger.info("model.download.attempt", model=model, attempt=attempt, mirror=mirror)
    return mirror


async def _attempt_mirror(mirror: str, spec: DownloadSpec, state: _State) -> None:
    """用指定镜像依次下载规格声明的全部文件（先探测权重总量，作为进度基准）。

    Raises:
        TimeoutError / httpx.HTTPError / OSError / ModelDownloadError:
            由 :func:`_download_all` 捕获后切换下一个镜像。
    """
    async with httpx.AsyncClient(
        follow_redirects=True, timeout=DOWNLOAD_TIMEOUT, verify=_ssl_context()
    ) as client:
        state.total_bytes = await _probe_size(client, _resolve_url(mirror, spec.repo, spec.weight))
        for name in spec.files:
            await _download_one(
                client,
                mirror,
                spec.repo,
                name,
                spec.root / name,
                state,
                count_progress=name == spec.weight,
            )


def _mark_ready(state: _State, model: str, mirror: str, attempt: int) -> None:
    """标记 ready（全部文件校验通过）并记录日志。"""
    state.status = _STATUS_READY
    state.error = None
    state.updated_at = _now_iso()
    logger.info("model.download.ready", model=model, mirror=mirror, attempt=attempt)


def _mark_failed(state: _State, model: str, last_error: str) -> None:
    """标记 failed（自动重试用尽，等待用户手动重试）并记录日志。"""
    state.status = _STATUS_FAILED
    state.error = f"下载失败（已自动重试 {MAX_ATTEMPTS} 次，切换镜像均未成功）: {last_error}"
    state.updated_at = _now_iso()
    logger.warning("model.download.failed", model=model, error=state.error)


async def _download_all(model: str) -> None:
    """下载主体：按镜像序列重试 MAX_ATTEMPTS 次，全失败则置 failed。

    本函数为后台任务，任何异常都收敛为 ``failed`` 状态（不向上抛），
    与探测类接口一致：状态查询不应让调用方 5xx。
    """
    state = _state_for(model)
    spec = resolve_spec(model)
    last_error: str = "未知错误"

    for attempt in range(1, MAX_ATTEMPTS + 1):
        mirror = _start_attempt(state, model, attempt)
        try:
            await _attempt_mirror(mirror, spec, state)
        except (TimeoutError, httpx.HTTPError, OSError, ModelDownloadError) as exc:
            last_error = f"{type(exc).__name__}: {exc}"
            logger.warning(
                "model.download.attempt_failed",
                model=model,
                attempt=attempt,
                mirror=mirror,
                error=last_error,
            )
            _cleanup_partials(spec.root, spec.files)
            continue

        if model_ready(model):
            _mark_ready(state, model, mirror, attempt)
            return
        last_error = "下载完成后文件校验未通过"
        _cleanup_partials(spec.root, spec.files)

    _mark_failed(state, model, last_error)


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
        httpx.HTTPError: 网络/状态码失败（由 :func:`_download_all` 捕获切换镜像）。
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
