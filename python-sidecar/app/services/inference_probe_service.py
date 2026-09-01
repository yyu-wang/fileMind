"""Ollama 推理环境探测服务（T6.7）。

探测本地 Ollama 可用性 + 已安装生成模型列表 + Embedding 模型可用性，
供设置页 / 引导页展示真实环境。对齐 API 规格书 §3.5 ``POST /inference/test``。

与 embedding_service 的区别：本服务只读 ``/api/tags``，不做向量化；即使
Ollama 不可用也返回结构化结果（``available=false``），不抛异常——探测是
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
    ModelInstallResponse,
    OllamaModelInfo,
    _OllamaTagModel,
    _OllamaTagsResponse,
)
from app.services.embedding_service import _OLLAMA_MODEL_ALIASES, OLLAMA_HOST

#: /api/tags 探测超时（秒）——轻量查询，快速失败避免设置页卡住
PROBE_TIMEOUT = float(os.environ.get("FILEMIND_PROBE_TIMEOUT", "3"))

logger = getLogger()
#: Ollama 模型名尾部标签后缀（比对前剥离，避免 :latest 干扰）
_LATEST_SUFFIX = ":latest"


async def probe_ollama() -> InferenceTestResponse:
    """探测本地 Ollama：可用性 + 生成模型列表 + Embedding 模型可用性。

    Ollama 不可用 / 响应异常时返回 ``available=false``（HTTP 200），不抛异常；
    单个 Embedding 模型未安装（未 ``ollama pull``）→ 该项 ``available=false``。

    Returns:
        探测结果（available / llm_models / embedding_models / error_code / message）。
    """
    try:
        tags = await _fetch_ollama_tags()
    except (httpx.HTTPError, ValidationError) as exc:
        return _unavailable(f"Ollama 探测失败: {exc}")

    installed_names = {_strip_tag(model.name) for model in tags}
    return InferenceTestResponse(
        available=True,
        status="ok",
        llm_models=[_to_ollama_model_info(model) for model in tags if _is_llm_model(model)],
        embedding_models=[
            _to_embedding_availability(name, installed_names) for name in sorted(MODEL_REGISTRY)
        ],
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
    """判断是否生成（LLM）模型：排除 Embedding 注册表名及其 Ollama 别名。"""
    stripped = _strip_tag(model.name)
    return stripped not in MODEL_REGISTRY and stripped not in _OLLAMA_MODEL_ALIASES.values()


def _to_ollama_model_info(model: _OllamaTagModel) -> OllamaModelInfo:
    """把 /api/tags 条目映射为对外模型信息（剥离标签、容忍 details 缺失）。"""
    return OllamaModelInfo(
        name=_strip_tag(model.name),
        size_bytes=model.size,
        family=model.details.family if model.details else None,
        modified_at=model.modified_at or None,
    )


def _to_embedding_availability(model_name: str, installed: set[str]) -> EmbeddingModelAvailability:
    """按注册表模型名检查其 Ollama 模型是否已安装（原名或别名单一命中即可）。

    注册表名（``bge-small-zh-v1.5``）与社区命名空间别名
    （``qllama/bge-small-zh-v1.5``）任一安装即视为可用——用户两种拉取
    方式（官方库原名 / 社区命名空间）都不应被误判为"未安装"。
    """
    info = get_model_info(model_name)
    candidates = {model_name, _OLLAMA_MODEL_ALIASES.get(model_name, model_name)}
    return EmbeddingModelAvailability(
        name=model_name,
        dim=info.dim,
        version=info.default_version,
        available=any(name in installed for name in candidates),
    )


def _unavailable(message: str) -> InferenceTestResponse:
    """构造 Ollama 不可用时的响应（Embedding 全标记为不可用）。"""
    return InferenceTestResponse(
        available=False,
        status="unavailable",
        llm_models=[],
        embedding_models=[
            EmbeddingModelAvailability(
                name=name,
                dim=get_model_info(name).dim,
                version=get_model_info(name).default_version,
                available=False,
            )
            for name in sorted(MODEL_REGISTRY)
        ],
        error_code="OLLAMA_UNAVAILABLE",
        message=message,
    )


#: /api/pull 拉取超时（秒）——模型下载可能较慢，给足预算
PULL_TIMEOUT = float(os.environ.get("FILEMIND_PULL_TIMEOUT", "600"))


async def install_embedding_model(model_name: str) -> ModelInstallResponse:
    """从 Ollama 拉取指定 Embedding 模型。

    Args:
        model_name: 注册表模型名（如 ``bge-small-zh-v1.5``）。

    Returns:
        安装结果。

    Raises:
        ValueError: 模型名不在注册表中。
        httpx.HTTPError: Ollama 请求失败。
    """
    if model_name not in MODEL_REGISTRY:
        raise ValueError(f"未知 Embedding 模型: {model_name!r}")

    ollama_name = _OLLAMA_MODEL_ALIASES.get(model_name, model_name)
    logger.info("inference_probe.install_model.start", model=model_name, ollama_name=ollama_name)

    try:
        async with httpx.AsyncClient(timeout=PULL_TIMEOUT) as client:
            resp = await client.post(
                f"{OLLAMA_HOST}/api/pull",
                json={"name": ollama_name, "stream": False},
            )
            resp.raise_for_status()
            body = resp.json()
        logger.info(
            "inference_probe.install_model.done",
            model=model_name,
            status=body.get("status", "unknown"),
        )
        return ModelInstallResponse(
            success=True,
            model_name=model_name,
            ollama_name=ollama_name,
            message=body.get("status", "success"),
        )
    except httpx.HTTPStatusError as exc:
        logger.error(
            "inference_probe.install_model.http_error",
            model=model_name,
            status=exc.response.status_code,
        )
        return ModelInstallResponse(
            success=False,
            model_name=model_name,
            ollama_name=ollama_name,
            message=f"Ollama 返回错误: HTTP {exc.response.status_code}",
        )
    except httpx.HTTPError as exc:
        logger.error("inference_probe.install_model.error", model=model_name, error=str(exc))
        return ModelInstallResponse(
            success=False,
            model_name=model_name,
            ollama_name=ollama_name,
            message=f"连接 Ollama 失败: {exc}",
        )
