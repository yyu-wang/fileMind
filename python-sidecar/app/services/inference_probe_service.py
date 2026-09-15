"""Ollama 推理环境探测服务（T6.7）。

探测本地 Ollama 可用性 + 已安装生成模型列表 + Embedding 模型文件就绪状态，
供设置页 / 引导页展示真实环境。对齐 API 规格书 §3.5 ``POST /inference/test``。

职责边界：
- Ollama 只负责**本地生成模型**（``llm_models``）；探测只读 ``/api/tags``，
  不对 Ollama 做任何写操作（历史上此处的 ``ollama pull`` 安装能力已随
  Embedding 进程内化移除）。
- Embedding 模型的 ``available`` 表示**本地模型文件是否就绪**，与 Ollama 无关
  （进程内 ONNX 推理，模型由设置页下载，见 ``model_download_service``）。
- 即使 Ollama 不可用也返回结构化结果（``available=false``），不抛异常——探测是
  轻量状态查询，不应让整个应用 5xx。
"""

from __future__ import annotations

import os

import httpx
from pydantic import ValidationError

from app.core.embedding_models import MODEL_REGISTRY, get_model_info
from app.core.logging import getLogger
from app.models import (
    EmbeddingModelAvailability,
    InferenceTestResponse,
    OllamaModelInfo,
    _OllamaTagModel,
    _OllamaTagsResponse,
)
from app.rules.llm_classify import OLLAMA_HOST
from app.services import model_download_service

#: /api/tags 探测超时（秒）——轻量查询，快速失败避免设置页卡住
PROBE_TIMEOUT = float(os.environ.get("FILEMIND_PROBE_TIMEOUT", "3"))

logger = getLogger()
#: Ollama 模型名尾部标签后缀（比对前剥离，避免 :latest 干扰）
_LATEST_SUFFIX = ":latest"
#: Ollama 侧可能残留的 Embedding 模型名（社区命名空间托底的 bge 系列）。
#: **仅**用于把它们从「生成模型」列表里排除：用户历史上可能用 ``ollama pull``
#: 装过，它们不是生成模型，不应出现在生成模型下拉里。
#: （Embedding 本身已改为进程内 ONNX 推理，此处不再参与可用性判定。）
_OLLAMA_EMBEDDING_NAMES: frozenset[str] = frozenset(
    {
        "qllama/bge-large-zh-v1.5",
        "qllama/bge-small-zh-v1.5",
        "awenleven/bge-m3:567m",
    }
)


async def probe_ollama() -> InferenceTestResponse:
    """探测本地 Ollama：可用性 + 生成模型列表 + Embedding 模型就绪状态。

    Ollama 不可用 / 响应异常时返回 ``available=false``（HTTP 200），不抛异常。

    注意：``embedding_models[].available`` 表示**本地模型文件是否就绪**，与 Ollama
    无关（Embedding 已改为 Sidecar 进程内 ONNX 推理，模型由设置页下载）。
    Ollama 只决定本地生成模型（``llm_models``）的可用性。

    Returns:
        探测结果（available / llm_models / embedding_models / error_code / message）。
    """
    try:
        tags = await _fetch_ollama_tags()
    except (httpx.HTTPError, ValidationError) as exc:
        return _unavailable(f"Ollama 探测失败: {exc}")

    return InferenceTestResponse(
        available=True,
        status="ok",
        llm_models=[_to_ollama_model_info(model) for model in tags if _is_llm_model(model)],
        embedding_models=_embedding_availability_list(),
        error_code=None,
        message=None,
    )


async def _fetch_ollama_tags() -> list[_OllamaTagModel]:
    """请求 Ollama ``/api/tags`` 并解析为模型列表。

    Raises:
        httpx.HTTPError: 连接失败 / 超时 / 非 2xx。
        pydantic.ValidationError: 响应结构不符合预期。
    """
    async with httpx.AsyncClient(timeout=PROBE_TIMEOUT) as client:
        resp = await client.get(f"{OLLAMA_HOST}/api/tags")
        resp.raise_for_status()
        # SC-m17：Ollama 返回非 JSON（如 HTML 错误页）时不应 500
        try:
            body = resp.json()
        except Exception:
            logger.warning("inference_probe.invalid_json", status=resp.status_code)
            return []
        return _OllamaTagsResponse.model_validate(body).models


def _strip_tag(name: str) -> str:
    """剥离模型名尾部的 ``:latest`` 等标签，便于与注册表名/别名比对。"""
    return name[: -len(_LATEST_SUFFIX)] if name.endswith(_LATEST_SUFFIX) else name


def _is_llm_model(model: _OllamaTagModel) -> bool:
    """判断是否生成（LLM）模型：排除 Embedding 注册表名与其 Ollama 残留名。"""
    stripped = _strip_tag(model.name)
    return stripped not in MODEL_REGISTRY and stripped not in _OLLAMA_EMBEDDING_NAMES


def _to_ollama_model_info(model: _OllamaTagModel) -> OllamaModelInfo:
    """把 /api/tags 条目映射为对外模型信息（剥离标签、容忍 details 缺失）。"""
    return OllamaModelInfo(
        name=_strip_tag(model.name),
        size_bytes=model.size,
        family=model.details.family if model.details else None,
        modified_at=model.modified_at or None,
    )


def _embedding_availability_list() -> list[EmbeddingModelAvailability]:
    """按注册表列出 Embedding 模型及「本地模型文件是否就绪」。

    就绪判定与 Ollama 无关：Embedding 由 Sidecar 进程内 ONNX 推理完成，模型文件
    由设置页触发下载（见 :mod:`app.services.model_download_service`）。
    """
    return [
        EmbeddingModelAvailability(
            name=name,
            dim=get_model_info(name).dim,
            version=get_model_info(name).default_version,
            available=model_download_service.model_ready(name),
        )
        for name in sorted(MODEL_REGISTRY)
    ]


def _unavailable(message: str) -> InferenceTestResponse:
    """构造 Ollama 不可用时的响应。

    Embedding 可用性与 Ollama 无关（进程内 ONNX 推理），故该项仍按本地模型文件
    判定，不随 Ollama 一起标为不可用。
    """
    return InferenceTestResponse(
        available=False,
        status="unavailable",
        llm_models=[],
        embedding_models=_embedding_availability_list(),
        error_code="OLLAMA_UNAVAILABLE",
        message=message,
    )
