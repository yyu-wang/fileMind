"""T5.2 — Embedding Top-k 召回率测试（08-§8 门控）。

生成 50 篇中文主题文档 + 10 条带标注查询，经 :mod:`app.services.embedding_service`
向量化后用余弦相似度排序，统计 Top-5 召回率；总体 > 80% 判定达标（Epic 5
混合检索准入门禁）。

用法：
    python -m eval.recall                       # bge-large-zh-v1.5，50 文档×10 查询 Top-5 > 80%
    python -m eval.recall --model bge-m3        # 备选模型复测
    python -m eval.recall --top-k 3 --threshold 0.7
    python -m eval.recall --corpus /tmp/recall.jsonl   # 同时导出语料

查询向量化按 BAAI bge-large-zh-v1.5 模型卡对检索短查询加指令前缀
（``QUERY_INSTRUCTION``），文档不加。
"""

from __future__ import annotations

import argparse
import asyncio
import json
import math
import random
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from collections.abc import Sequence

from app.services.embedding_service import (
    EMBEDDING_MODEL,
    QUERY_INSTRUCTION,
    embed_text,
    embed_texts,
)

#: 主题：(键, 中文标题, 句子池)。每主题 5 篇文档 + 1 条查询。
_TOPICS: tuple[tuple[str, str, tuple[str, ...]], ...] = (
    (
        "finance",
        "财务报表",
        (
            "公司本季度营收较上季度增长12%，主要来自核心产品线放量。",
            "净利润率保持稳定，毛利率达到38%，现金流状况良好。",
            "资产负债表显示应收账款周转加快，经营现金流为正。",
            "审计委员会审阅了季度报告，未发现重大异常调整项。",
            "管理层预计下季度营收区间在5-6亿元，环比小幅增长。",
            "财务总监汇报了折旧摊销与资本开支的预算执行情况。",
            "季度利润表与现金流量表勾稽关系核对一致，符合会计准则。",
        ),
    ),
    (
        "arch",
        "技术架构",
        (
            "服务采用微服务架构，通过 API 网关统一接入鉴权与限流。",
            "缓存层使用 Redis 承载热点数据，数据库分库分表缓解压力。",
            "消息队列解耦了订单与库存系统，保证最终一致性。",
            "部署采用容器化方案，Kubernetes 自动扩缩容保障高可用。",
            "接口设计遵循 RESTful 规范，统一错误码与幂等策略。",
            "监控平台覆盖了调用链追踪、日志采集与告警规则。",
        ),
    ),
    (
        "product",
        "产品需求",
        (
            "用户故事聚焦核心场景，按 MoSCoW 原则划分优先级。",
            "功能清单已确认，原型图通过需求评审进入迭代排期。",
            "用户调研显示注册转化率是主要优化点，纳入本次迭代。",
            "产品经理整理了需求池，标注了每项的收益与成本估算。",
            "验收标准定义清晰，开发前需求冻结避免范围蔓延。",
            "埋点方案覆盖核心漏斗，用于评估新功能上线效果。",
        ),
    ),
    (
        "market",
        "市场竞品",
        (
            "竞品分析报告对比了三家主要厂商的功能与定价策略。",
            "目标用户画像以中小企业为主，预算敏感度高。",
            "渠道策略侧重线上投放与合作伙伴生态共建。",
            "市场份额数据显示行业集中度上升，头部效应明显。",
            "营销活动以内容营销和案例分享驱动线索转化。",
            "SWOT 分析指出差异化优势在于本地化服务与响应速度。",
        ),
    ),
    (
        "hr",
        "人力资源",
        (
            "招聘计划覆盖后端与产品岗位，简历初筛已启动。",
            "面试流程包含技术面与 HR 面，一周内反馈结果。",
            "绩效评估采用目标与关键结果对齐的机制。",
            "薪酬调研完成，新职级带宽已内部公示。",
            "培训课程围绕新员工入职与安全合规展开。",
            "员工满意度调研显示晋升通道是关注重点。",
        ),
    ),
    (
        "project",
        "项目管理",
        (
            "项目里程碑按季度拆解，关键路径明确到周。",
            "风险登记册记录了供应商延迟与人员变动的应对方案。",
            "任务分解结构覆盖需求、开发、测试与发布阶段。",
            "每周站会同步进度，燃尽图显示迭代基本在轨。",
            "验收标准与发布计划已对齐，预留灰度与回滚窗口。",
            "资源分配表平衡了各团队的负载，避免瓶颈阻塞。",
        ),
    ),
    (
        "qa",
        "研发测试",
        (
            "测试用例覆盖核心业务链路，冒烟测试每日执行。",
            "回归测试通过率 98%，剩余缺陷均为低优先级。",
            "自动化测试脚本纳入流水线，覆盖率目标 80%。",
            "缺陷管理系统按严重级别分派，阻塞问题当日修复。",
            "测试环境与生产隔离，灰度环境用于验证新特性。",
            "性能测试报告显示接口 P95 延迟 120ms，符合 SLO。",
        ),
    ),
    (
        "support",
        "客户服务",
        (
            "工单系统跟踪售后请求，平均响应时间 2 小时。",
            "客服团队按客户等级分级处理，重点客户优先跟进。",
            "满意度回访显示解决率提升，投诉集中在计费模块。",
            "常见问题知识库持续更新，减少重复工单。",
            "客户成功团队定期回访，收集产品改进建议。",
            "应急预案覆盖故障公告与补偿方案，降低负面影响。",
        ),
    ),
    (
        "meeting",
        "会议纪要",
        (
            "会议议题包括版本规划、资源协调与风险同步。",
            "会上确认了三项结论，并拆解为明确待办与负责人。",
            "参会人包括产品、研发与运营，记录由行政归档。",
            "讨论聚焦下季度目标，决议形成书面纪要并邮件分发。",
            "遗留问题列入下期议程，由专项小组跟进闭环。",
            "纪要及时同步给未参会同事，确保信息透明一致。",
        ),
    ),
    (
        "security",
        "数据安全",
        (
            "隐私政策已更新，数据处理遵循最小必要原则。",
            "敏感字段加密存储，访问权限按角色最小化授予。",
            "审计日志记录关键操作，支持合规追溯。",
            "数据脱敏方案覆盖生产与测试环境，防止泄露。",
            "合规检查通过，满足 GDPR 与本地数据保护法规要求。",
            "安全评审确认外部接口鉴权与传输加密无缺口。",
        ),
    ),
)

#: 每主题查询（自然问句）
_QUERIES: tuple[str, ...] = (
    "公司本季度的营收、利润和现金流表现如何？",
    "系统的高可用架构和微服务部署方案是怎么设计的？",
    "产品迭代的需求优先级和功能清单是如何确定的？",
    "竞品分析结果显示市场格局和用户画像有什么特点？",
    "公司目前的招聘、绩效和培训体系运行情况如何？",
    "项目的排期里程碑和风险应对计划是怎样的？",
    "研发测试的用例覆盖和回归通过率情况如何？",
    "客户服务的工单响应和满意度改善措施有哪些？",
    "本次会议确认了哪些结论和待办事项？",
    "数据合规与隐私保护采取了哪些安全措施？",
)


@dataclass(frozen=True)
class RecallDoc:
    """单篇测试文档（doc_id 以主题键为前缀）。"""

    doc_id: str
    title: str
    content: str


@dataclass(frozen=True)
class RecallQuery:
    """单条测试查询，``relevant_ids`` 为标注的相关文档。"""

    query_id: str
    query: str
    topic: str
    relevant_ids: tuple[str, ...]


@dataclass(frozen=True)
class QueryRecall:
    """单条查询的召回结果。"""

    query_id: str
    topic: str
    recall: float
    hits: int
    relevant_count: int
    top_docs: tuple[str, ...]


@dataclass(frozen=True)
class RecallReport:
    """一次召回测试的汇总结果。"""

    docs: tuple[RecallDoc, ...]
    queries: tuple[RecallQuery, ...]
    per_query: tuple[QueryRecall, ...]
    overall_recall: float
    top_k: int
    threshold: float


def generate_corpus(docs_per_topic: int = 5, seed: int = 42) -> tuple[RecallDoc, ...]:
    """确定性生成测试文档：每主题 ``docs_per_topic`` 篇。

    文档 = 标题 + 按 doc 索引轮转选取的 3 句主题句（同主题文档句子有重叠、
    跨主题句子完全不同），保证可检索性。

    Args:
        docs_per_topic: 每主题文档数（默认 5，即 50 篇）。
        seed: 随机种子（默认 42），保证可复现。

    Returns:
        文档列表。
    """
    rng = random.Random(seed)
    docs: list[RecallDoc] = []
    for topic_key, title, sentences in _TOPICS:
        for i in range(docs_per_topic):
            picked = [sentences[(i + offset) % len(sentences)] for offset in range(3)]
            docs.append(
                RecallDoc(
                    doc_id=f"{topic_key}_doc{i}",
                    title=f"{title}文档{i + 1}",
                    content="\n".join(picked),
                )
            )
    # 句子池顺序洗牌只影响内容文字，不影响主题归属（保证可复现）
    rng.shuffle(docs)
    return tuple(docs)


def generate_queries(docs: Sequence[RecallDoc]) -> tuple[RecallQuery, ...]:
    """为每个主题生成一条查询，标注相关文档为该主题的全部文档。

    Args:
        docs: 测试文档列表（doc_id 以主题键为前缀）。

    Returns:
        查询列表，与主题顺序一致。
    """
    queries: list[RecallQuery] = []
    for index, (topic_key, title, _sentences) in enumerate(_TOPICS):
        relevant = tuple(d.doc_id for d in docs if d.doc_id.startswith(f"{topic_key}_doc"))
        queries.append(
            RecallQuery(
                query_id=f"q{index}",
                query=_QUERIES[index],
                topic=title,
                relevant_ids=relevant,
            )
        )
    return tuple(queries)


def _cosine(a: Sequence[float], b: Sequence[float]) -> float:
    """余弦相似度；任一向量为零向量时返回 0。"""
    dot = sum(x * y for x, y in zip(a, b, strict=False))
    norm_a = math.sqrt(sum(x * x for x in a))
    norm_b = math.sqrt(sum(y * y for y in b))
    if norm_a == 0 or norm_b == 0:
        return 0.0
    return dot / (norm_a * norm_b)


async def run_recall(
    docs: Sequence[RecallDoc],
    queries: Sequence[RecallQuery],
    model: str = EMBEDDING_MODEL,
    top_k: int = 5,
    threshold: float = 0.80,
) -> RecallReport:
    """执行召回测试：文档批量向量化 + 逐查询余弦排序 + Top-k 召回统计。

    Args:
        docs: 测试文档。
        queries: 测试查询。
        model: Embedding 模型名（默认 ``EMBEDDING_MODEL``）。
        top_k: 截断候选数（默认 5）。
        threshold: 达标门禁（默认 0.80，仅用于报告判定）。

    Returns:
        汇总报告（含每查询召回与总体召回率）。
    """
    doc_texts = [f"{d.title}\n{d.content}" for d in docs]
    doc_vecs = await embed_texts(list(doc_texts), model=model)

    per_query: list[QueryRecall] = []
    for q in queries:
        query_vec = await embed_text(QUERY_INSTRUCTION + q.query, model=model)
        similarities = [_cosine(query_vec, doc_vec) for doc_vec in doc_vecs]
        ranked = sorted(range(len(docs)), key=lambda i: similarities[i], reverse=True)[:top_k]
        top_ids = tuple(docs[i].doc_id for i in ranked)
        hits = sum(1 for did in top_ids if did in q.relevant_ids)
        per_query.append(
            QueryRecall(
                query_id=q.query_id,
                topic=q.topic,
                recall=hits / len(q.relevant_ids),
                hits=hits,
                relevant_count=len(q.relevant_ids),
                top_docs=top_ids,
            )
        )

    overall = sum(p.recall for p in per_query) / len(per_query) if per_query else 0.0
    return RecallReport(
        docs=tuple(docs),
        queries=tuple(queries),
        per_query=tuple(per_query),
        overall_recall=overall,
        top_k=top_k,
        threshold=threshold,
    )


def format_recall_report(report: RecallReport) -> str:
    """渲染召回评估报告为可读文本。"""
    lines = [
        "=== Embedding 召回率评估报告 ===",
        f"文档数: {len(report.docs)}  查询数: {len(report.queries)}  "
        f"Top-{report.top_k}  门禁: {report.threshold:.0%}",
        f"总体召回率: {report.overall_recall:.1%}",
        "",
        "按查询召回率:",
    ]
    for p in report.per_query:
        lines.append(
            f"  {p.query_id:<3} {p.topic:<6} 命中={p.hits}/{p.relevant_count} 召回={p.recall:.1%}"
        )
    return "\n".join(lines)


def write_corpus_jsonl(
    path: Path, docs: Sequence[RecallDoc], queries: Sequence[RecallQuery]
) -> None:
    """导出测试语料与查询标注为 JSONL（便于人工核查）。"""
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as fh:
        for doc in docs:
            fh.write(json.dumps(asdict(doc), ensure_ascii=False) + "\n")
        for q in queries:
            fh.write(json.dumps(asdict(q), ensure_ascii=False) + "\n")


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    """解析 CLI 参数。"""
    parser = argparse.ArgumentParser(
        prog="python -m eval.recall",
        description="Embedding Top-k 召回率测试（08-§8 门控：50文档×10查询 Top-5>80%）",
    )
    parser.add_argument(
        "--model", default=EMBEDDING_MODEL, help="Embedding 模型名（默认注册表默认值）"
    )
    parser.add_argument("--docs", type=int, default=50, help="文档总数（默认 50，=10 主题×5）")
    parser.add_argument("--top-k", type=int, default=5, help="截断候选数（默认 5）")
    parser.add_argument("--threshold", type=float, default=0.80, help="总体召回率门禁（默认 0.80）")
    parser.add_argument("--seed", type=int, default=42, help="随机种子（默认 42）")
    parser.add_argument("--corpus", type=Path, default=None, help="导出语料 JSONL 路径（可选）")
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    """CLI 入口：生成语料 → 跑召回 → 打印报告 → 门禁退出码。"""
    args = parse_args(argv)
    docs_per_topic = max(1, args.docs // len(_TOPICS))
    docs = generate_corpus(docs_per_topic=docs_per_topic, seed=args.seed)
    queries = generate_queries(docs)

    if args.corpus is not None:
        write_corpus_jsonl(args.corpus, docs, queries)
        print(f"[recall] 已导出语料: {args.corpus}")

    report = asyncio.run(run_recall(docs, queries, model=args.model, top_k=args.top_k))
    print(format_recall_report(report))

    if report.overall_recall > args.threshold:
        print(f"✓ 召回率达标（门禁 {args.threshold:.0%}）")
        return 0
    print(f"✗ 召回率未达标（门禁 {args.threshold:.0%}），建议按 08-§8 测试备选模型 bge-m3")
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
