"""文档文本抽取（预览）请求/响应模型。"""

from pydantic import BaseModel, Field


class DocumentExtractRequest(BaseModel):
    """请求 Sidecar 抽取二进制文档正文。

    Attributes:
        path: 待抽取文档的绝对路径（调用前已由 Rust ``security::validate`` 校验）。
    """

    path: str = Field(min_length=1, description="待抽取文档的绝对路径")


class DocumentExtractResponse(BaseModel):
    """文档文本抽取结果。

    Attributes:
        text: 抽取出的纯文本（office/PDF；上限与 doc_extract 一致）。
    """

    text: str = Field(description="抽取出的纯文本内容")
