"""推理环境探测路由（API 规格书 §3.5）。

只保留探测端点：Ollama 不可用时返回 ``available=false`` 的结构化结果，由前端
展示友好提示（对齐错误码 OLLAMA_UNAVAILABLE 的语义）。

说明：历史上的 ``POST /inference/install-model``（从 Ollama 拉取 Embedding 模型）
已随 Embedding 进程内化移除——模型改由 ``/models/download`` 从 HF 镜像下载。
"""

from fastapi import APIRouter

from app.models import InferenceTestResponse
from app.services.inference_probe_service import probe_ollama

router = APIRouter(prefix="/inference", tags=["推理"])


@router.post("/test", response_model=InferenceTestResponse)
async def inference_test() -> InferenceTestResponse:
    """探测本地 Ollama：可用性 + 生成模型列表 + Embedding 模型文件就绪状态。

    Returns:
        结构化探测结果（Ollama 不可用也是 HTTP 200，见 ``available`` 字段）。
    """
    return await probe_ollama()
