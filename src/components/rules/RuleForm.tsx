// 规则新建 / 编辑表单（T6.8）。
//
// 规则类型仅开放后端 `match_rule` 已支持的 extension / path_keyword / regex；
// magic_number / size 以禁用占位呈现，避免用户创建永不生效的规则。

import { useState } from 'react';
import type { Category, Rule } from '../../types/ipc';
import { RULE_TYPE_META, RuleType } from '../../types/models';

interface RuleFormProps {
  /** 编辑的规则（null 表示新建） */
  initial: Rule | null;
  /** 目标分类下拉数据 */
  categories: Category[];
  /** 提交保存 */
  onSave: (rule: Rule) => void;
  /** 取消 */
  onCancel: () => void;
}

const RULE_TYPE_OPTIONS = (Object.keys(RULE_TYPE_META) as RuleType[]).map((value) => ({
  value,
  label: RULE_TYPE_META[value].label,
  disabled: RULE_TYPE_META[value].disabled,
}));

export function RuleForm({ initial, categories, onSave, onCancel }: RuleFormProps) {
  const [name, setName] = useState(initial?.name ?? '');
  const [ruleType, setRuleType] = useState<RuleType>(
    (initial?.rule_type as RuleType) ?? RuleType.Extension,
  );
  const [pattern, setPattern] = useState(initial?.pattern ?? '');
  const [targetCategory, setTargetCategory] = useState(initial?.target_category ?? '');
  const [priority, setPriority] = useState<number>(initial?.priority ?? 100);
  const [error, setError] = useState<string | null>(null);

  const meta = RULE_TYPE_META[ruleType];

  const handleSubmit = () => {
    if (!name.trim()) {
      setError('规则名不能为空');
      return;
    }
    if (!pattern.trim()) {
      setError('匹配模式不能为空');
      return;
    }
    if (ruleType === RuleType.Regex) {
      // 前端预校验正则合法性，避免后端执行期才报错
      try {
        RegExp(pattern);
      } catch {
        setError('正则表达式不合法');
        return;
      }
    }

    onSave({
      id: initial?.id ?? '',
      name: name.trim(),
      rule_type: ruleType,
      pattern: pattern.trim(),
      target_category: targetCategory || null,
      priority,
      is_enabled: initial?.is_enabled ?? true,
      created_at: initial?.created_at ?? '',
      updated_at: initial?.updated_at ?? '',
    });
  };

  return (
    <div className="rules-form" role="dialog" aria-modal="true" aria-label="规则编辑">
      <div className="rules-form__panel">
        <h3 className="rules-form__title">{initial ? '编辑规则' : '新建规则'}</h3>

        {error && (
          <div className="rules-form__error" role="alert">
            {error}
          </div>
        )}

        <div className="rules-form__field">
          <label className="rules-form__label" htmlFor="rule-name">
            规则名
          </label>
          <input
            id="rule-name"
            className="rules-form__input"
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="如：PDF 文档归档"
            autoFocus
          />
        </div>

        <div className="rules-form__field">
          <label className="rules-form__label" htmlFor="rule-type">
            规则类型
          </label>
          <select
            id="rule-type"
            className="rules-form__select"
            value={ruleType}
            onChange={(e) => setRuleType(e.target.value as RuleType)}
          >
            {RULE_TYPE_OPTIONS.map((opt) => (
              <option key={opt.value} value={opt.value} disabled={opt.disabled}>
                {opt.label}
                {opt.disabled ? '（即将推出）' : ''}
              </option>
            ))}
          </select>
        </div>

        <div className="rules-form__field">
          <label className="rules-form__label" htmlFor="rule-pattern">
            匹配模式
          </label>
          <input
            id="rule-pattern"
            className="rules-form__input"
            value={pattern}
            onChange={(e) => setPattern(e.target.value)}
            placeholder={meta.hint}
          />
          <p className="rules-form__hint">{meta.hint}</p>
        </div>

        <div className="rules-form__field">
          <label className="rules-form__label" htmlFor="rule-category">
            目标分类
          </label>
          <select
            id="rule-category"
            className="rules-form__select"
            value={targetCategory}
            onChange={(e) => setTargetCategory(e.target.value)}
          >
            <option value="">不指定（仅打标签不移动）</option>
            {categories.map((c) => (
              <option key={c.id} value={c.id}>
                {c.name}
              </option>
            ))}
          </select>
        </div>

        <div className="rules-form__field">
          <label className="rules-form__label" htmlFor="rule-priority">
            优先级
          </label>
          <input
            id="rule-priority"
            className="rules-form__input rules-form__input--number"
            type="number"
            value={priority}
            onChange={(e) => setPriority(Number(e.target.value))}
          />
          <p className="rules-form__hint">数字越大越先匹配（也可在列表中拖拽排序）</p>
        </div>

        <div className="rules-form__actions">
          <button type="button" className="btn btn--ghost" onClick={onCancel}>
            取消
          </button>
          <button type="button" className="btn btn--primary" onClick={handleSubmit}>
            保存
          </button>
        </div>
      </div>
    </div>
  );
}
