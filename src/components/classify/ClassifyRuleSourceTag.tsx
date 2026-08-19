// 分类来源小标签：区分 规则命中 / 启发式 / 待确认（设计稿 §5 预览树）。
//
// 来源标签由 Rust 生成：`rule:<规则名>` / `heuristic` / `pending`。

import { PENDING_NAME } from '@/stores/classifyStore';

/** 来源标签前缀（与 Rust `RULE_SOURCE_PREFIX` 保持一致）。 */
const RULE_PREFIX = 'rule:';

interface ClassifyRuleSourceTagProps {
  /** 来源标签：`rule:<规则名>` / `heuristic` / `pending` */
  source: string;
}

/** 规则/启发式/待确认 三种来源的展示标签。 */
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
  return <span className="classify-rule-tag classify-rule-tag--pending">{PENDING_NAME}</span>;
}
