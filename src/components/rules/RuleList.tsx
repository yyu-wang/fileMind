// 规则列表（对齐交互原型 §规则编辑）：rules-list + rule-item（rule-name + rule-desc + rule-meta tag）。
//
// 文档结构（点击选中 → 详情面板渲染表单）：
//   <div class="rules-list">
//     <div class="rules-list-header"><h3>📋 规则列表</h3><span class="tag tag-blue rules-list-count">N 条</span></div>
//     <div class="rule-item active">
//       <div class="rule-main">
//         <div class="rule-name">…</div>
//         <div class="rule-desc">…</div>
//         <div class="rule-meta"><span class="tag tag-green">启用</span>…</div>
//       </div>
//       <span class="rule-priority">#N</span>
//     </div>
//     …
//   </div>
//
// 拖拽实现：onDragStart 记录拖动项，onDrop 时按目标位置重排并一次性提交 onReorder。

import { useState } from 'react';
import type { Category, Rule } from '../../types/ipc';
import { RULE_TYPE_META, type RuleType } from '../../types/models';

interface RuleListProps {
  rules: Rule[];
  categories: Category[];
  /** 当前选中规则 id（高亮 .active） */
  selectedId: string | null;
  /** 选中规则（点击 rule-item 触发） */
  onSelect: (rule: Rule) => void;
  /** 拖拽结束后的最终顺序（规则 id 数组） */
  onReorder: (orderedIds: string[]) => void;
}

/**
 * 规则简述：把 pattern + 目标分类拼成一句话，对齐文档 rule-desc 形态。
 *
 * 示例：
 *   - extension + pattern "pdf" + 分类"文档" → "扩展名 pdf → /文档/"
 *   - path_keyword + pattern "会议" → "文件名含 会议 → /会议/"
 *   - regex + pattern "^\d{4}-" → "正则 ^\d{4}- → /未指定/"
 */
function buildDescription(rule: Rule, categoryName: string): string {
  const typeMeta = RULE_TYPE_META[rule.rule_type as RuleType];
  const verb = typeMeta?.label ?? rule.rule_type;
  const target = categoryName === '—' ? '未指定' : `/${categoryName}/`;
  return `${verb} ${rule.pattern} → ${target}`;
}

export function RuleList({ rules, categories, selectedId, onSelect, onReorder }: RuleListProps) {
  const [dragId, setDragId] = useState<string | null>(null);

  const categoryName = (id: string | null): string =>
    id ? (categories.find((c) => c.id === id)?.name ?? '已删除分类') : '—';

  const handleDrop = (targetId: string) => {
    if (!dragId || dragId === targetId) return;
    const from = rules.findIndex((r) => r.id === dragId);
    const to = rules.findIndex((r) => r.id === targetId);
    if (from === -1 || to === -1) return;
    const next = [...rules];
    const [moved] = next.splice(from, 1);
    next.splice(to, 0, moved);
    onReorder(next.map((r) => r.id));
  };

  return (
    <div className="rules-list" data-testid="rule-list">
      <div className="rules-list-header">
        <h3>📋 规则列表</h3>
        <span className="tag tag-blue rules-list-count">{rules.length} 条</span>
      </div>

      {rules.map((rule) => {
        const isSelected = rule.id === selectedId;
        const isDragging = dragId === rule.id;
        return (
          <div
            key={rule.id}
            className={`rule-item${isSelected ? ' active' : ''}${isDragging ? ' rule-item--dragging' : ''}`}
            data-testid="rule-item"
            draggable
            onDragStart={() => setDragId(rule.id)}
            onDragOver={(e) => e.preventDefault()}
            onDrop={() => handleDrop(rule.id)}
            onDragEnd={() => setDragId(null)}
            onClick={() => onSelect(rule)}
            role="button"
            tabIndex={0}
            aria-pressed={isSelected}
            onKeyDown={(e) => {
              if (e.key === 'Enter' || e.key === ' ') {
                e.preventDefault();
                onSelect(rule);
              }
            }}
          >
            <div className="rule-main">
              <div className="rule-name">{rule.name}</div>
              <div className="rule-desc">
                {buildDescription(rule, categoryName(rule.target_category))}
              </div>
              <div className="rule-meta">
                {rule.is_enabled ? (
                  <span className="tag tag-green">启用</span>
                ) : (
                  <span className="tag tag-gray">已禁用</span>
                )}
                <span className="tag tag-purple">
                  {RULE_TYPE_META[rule.rule_type as RuleType]?.label ?? rule.rule_type}
                </span>
              </div>
            </div>
            <span className="rule-priority" title="优先级（数字越大越先匹配）">
              #{rule.priority}
            </span>
          </div>
        );
      })}
    </div>
  );
}
