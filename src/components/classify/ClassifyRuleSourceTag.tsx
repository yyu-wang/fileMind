// 分类来源小标签：区分 规则命中 / 启发式 / LLM 兜底 / 手动指定 / 待确认 / 待人工确认。
//
// 来源标签由 Rust 生成：`rule:<规则名>` / `heuristic` / `llm` / `needs_review` / `pending`；
// `manual` 为前端 T6.12 手动分类产生的本地标记。

import { PENDING_NAME } from '@/stores/classifyStore';

/** 来源标签前缀（与 Rust `RULE_SOURCE_PREFIX` 保持一致）。 */
const RULE_PREFIX = 'rule:';

interface ClassifyRuleSourceTagProps {
  /** 来源标签：`rule:<规则名>` / `heuristic` / `llm` / `manual` / `needs_review` / `pending` */
  source: string;
}

/** 规则/启发式/LLM 兜底/手动指定/待确认 的展示标签。 */
export function ClassifyRuleSourceTag({ source }: ClassifyRuleSourceTagProps) {
  if (source.startsWith(RULE_PREFIX)) {
    return (
      <span className="classify-rule-tag classify-rule-tag--rule">
        {source.slice(RULE_PREFIX.length)}
      </span>
    );
  }
  if (source === 'heuristic') {
    return <span className="classify-rule-tag classify-rule-tag--heuristic">启发式</span>;
  }
  if (source === 'llm') {
    return <span className="classify-rule-tag classify-rule-tag--llm">LLM 兜底</span>;
  }
  if (source === 'manual') {
    return <span className="classify-rule-tag classify-rule-tag--manual">手动指定</span>;
  }
  if (source === 'needs_review') {
    return <span className="classify-rule-tag classify-rule-tag--needs-review">待人工确认</span>;
  }
  return <span className="classify-rule-tag classify-rule-tag--pending">{PENDING_NAME}</span>;
}
