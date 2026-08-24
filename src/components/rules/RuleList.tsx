// 规则列表（T6.8）：原生 HTML5 拖拽排序 + 启用开关 + 编辑/删除入口。
//
// 拖拽实现：不维护本地列表副本（避免 state 与 props 同步），直接基于 props.rules
// 渲染；onDragStart 记录拖动项，onDrop 时按目标位置重排并一次性提交 onReorder。

import { useState } from 'react';
import type { Category, Rule } from '../../types/ipc';
import { RULE_TYPE_META, type RuleType } from '../../types/models';

interface RuleListProps {
  rules: Rule[];
  categories: Category[];
  onEdit: (rule: Rule) => void;
  onDelete: (rule: Rule) => void;
  /** 启用/禁用切换（传入待翻转的规则） */
  onToggle: (rule: Rule) => void;
  /** 拖拽结束后的最终顺序（规则 id 数组） */
  onReorder: (orderedIds: string[]) => void;
}

export function RuleList({
  rules,
  categories,
  onEdit,
  onDelete,
  onToggle,
  onReorder,
}: RuleListProps) {
  const [dragId, setDragId] = useState<string | null>(null);

  const categoryName = (id: string | null): string =>
    id ? (categories.find((c) => c.id === id)?.name ?? '已删除分类') : '—';

  const typeLabel = (t: string): string => RULE_TYPE_META[t as RuleType]?.label ?? t;

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
    <ul className="rules-list">
      {rules.map((rule) => (
        <li
          key={rule.id}
          className={`rules-item ${dragId === rule.id ? 'rules-item--dragging' : ''}`}
          data-testid="rule-item"
          draggable
          onDragStart={() => setDragId(rule.id)}
          onDragOver={(e) => e.preventDefault()}
          onDrop={() => handleDrop(rule.id)}
          onDragEnd={() => setDragId(null)}
        >
          <span className="rules-item__handle" aria-hidden>
            ⠿
          </span>

          <div className="rules-item__main">
            <div className="rules-item__name-row">
              <span className="rules-item__name">{rule.name}</span>
              <span className="rules-item__type">{typeLabel(rule.rule_type)}</span>
              {!rule.is_enabled && <span className="rules-item__disabled-tag">已禁用</span>}
            </div>
            <div className="rules-item__meta">
              <span className="rules-item__pattern" title={rule.pattern}>
                {rule.pattern}
              </span>
              <span className="rules-item__sep">→</span>
              <span className="rules-item__category">{categoryName(rule.target_category)}</span>
            </div>
          </div>

          <span className="rules-item__priority" title="优先级（数字越大越先匹配）">
            #{rule.priority}
          </span>

          <label className="rules-item__switch">
            <input
              type="checkbox"
              className="rules-item__switch-input"
              data-testid="rule-toggle"
              checked={rule.is_enabled}
              onChange={() => onToggle(rule)}
              aria-label={`${rule.is_enabled ? '禁用' : '启用'}规则 ${rule.name}`}
            />
            <span className="rules-item__switch-slider" aria-hidden />
          </label>

          <button
            type="button"
            className="btn rules-item__btn"
            onClick={() => onEdit(rule)}
            data-testid="rule-edit"
          >
            编辑
          </button>
          <button
            type="button"
            className="btn rules-item__btn rules-item__btn--danger"
            onClick={() => onDelete(rule)}
            data-testid="rule-delete"
          >
            删除
          </button>
        </li>
      ))}
    </ul>
  );
}
