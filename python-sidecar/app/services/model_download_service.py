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

镜像与重试策略（需求约定）：
- 镜像序列默认 ``hf-mirror.com`` → ``huggingface.co``（env 可覆盖）
- 自动尝试最多 :data:`MAX_ATTEMPTS`（默认 3）次，每次失败切换到下一个镜像
- 用尽后停止自动重试，状态置 ``failed``，等待用户在设置页手动点击重试
  （手动重试会重新计数）

状态语义（``status``）：``idle`` 未开始 / ``downloading`` 进行中 /
``ready`` 文件齐备 / ``failed`` 自动重试用尽。所有文件先写 ``.part`` 再原子
改名，避免半截文件被判定为就绪。

模块划分（原单文件 410 行，逼近 Python 模块 500 行强制阈值，见 `rules/complexity.md`）：
- :mod:`app.services.model_download_state` 状态数据类 / 错误类型 / 时间戳（叶子依赖）
- :mod:`app.services.model_download_transfer` HTTP 传输（SSL / 探测 / 流式下载 / 进度）
- 本模块：镜像重试编排 + 进程内状态表 + 对外 API
"""

from __future__ import annotations

import asyncio
import os
from typing import TYPE_CHECKING

import httpx

from app.core.logging import getLogger
from app.services.embedding_service import EmbeddingUnavailableError
from app.services.model_download_state import (
    _STATUS_DOWNLOADING,
    _STATUS_FAILED,
    _STATUS_READY,
    ModelDownloadError,
    _now_iso,
    _State,
)
from app.services.model_download_transfer import (
    _cleanup_partials,
    _download_one,
    _probe_size,
    _resolve_url,
    _ssl_context,
)
from app.services.model_specs import resolve_spec, spec_ready

if TYPE_CHECKING:
    from app.models import ModelDownloadStatusResponse
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
