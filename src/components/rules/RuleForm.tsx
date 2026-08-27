// 规则新建 / 编辑表单（对齐交互原型 §规则编辑 §rule-form）。
//
// 文档结构（rules-detail 内嵌，非模态弹窗）：
//   <div class="card">
//     <h3>规则名</h3>
//     <div class="rule-form">
//       <div class="form-group"><label>规则名称</label><input/></div>
//       <div class="form-group"><label>目标分类</label><select/></div>
//       <div class="form-group">
//         <label>匹配条件</label>
//         <div class="condition-builder">
//           <div class="condition-row">
//             <select>条件类型</select>
//             <input class="condition-row__input"/>
//           </div>
//         </div>
//         <span class="hint">…</span>
//       </div>
//       <div class="form-group"><label>优先级</label><input type="number"/></div>
//       <div class="setting-row">
//         <div class="setting-label"><div class="name">启用规则</div><div class="desc">…</div></div>
//         <div class="setting-control"><button class="toggle on"/></div>
//       </div>
//       <div class="rule-form__actions">
//         <button class="btn btn-primary">保存</button>
//         <button class="btn btn-ghost">取消</button>
//         <button class="btn btn-danger">删除</button>
//       </div>
//     </div>
//   </div>
//
// 后端约束：rule_type 仅支持 extension / path_keyword / regex；magic_number / size
// 以禁用占位呈现，避免用户创建永不生效的规则。多条件构造（AND/OR）需要后端
// 支持，当前仅渲染单 condition-row 与 rule_type/pattern 一对一映射。

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
  /** 取消（仅新建/未保存时使用，回到无选中态） */
  onCancel: () => void;
  /** 删除当前编辑的规则（新建态下隐藏） */
  onDelete?: (rule: Rule) => void;
}

const RULE_TYPE_OPTIONS = (Object.keys(RULE_TYPE_META) as RuleType[]).map((value) => ({
  value,
  label: RULE_TYPE_META[value].label,
  disabled: RULE_TYPE_META[value].disabled,
}));

export function RuleForm({ initial, categories, onSave, onCancel, onDelete }: RuleFormProps) {
  const [name, setName] = useState(initial?.name ?? '');
  const [ruleType, setRuleType] = useState<RuleType>(
    (initial?.rule_type as RuleType) ?? RuleType.Extension,
  );
  const [pattern, setPattern] = useState(initial?.pattern ?? '');
  const [targetCategory, setTargetCategory] = useState(initial?.target_category ?? '');
  const [priority, setPriority] = useState<number>(initial?.priority ?? 100);
  const [isEnabled, setIsEnabled] = useState<boolean>(initial?.is_enabled ?? true);
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
      is_enabled: isEnabled,
      created_at: initial?.created_at ?? '',
      updated_at: initial?.updated_at ?? '',
    });
  };

  const handleDelete = () => {
    if (initial && onDelete) {
      onDelete(initial);
    }
  };

  return (
    <div className="card" data-testid="rule-form">
      <h3>{initial ? initial.name || '编辑规则' : '新建规则'}</h3>

      <div className="rule-form">
        {error && (
          <div className="rule-form__error" role="alert">
            {error}
          </div>
        )}

        <div className="form-group">
          <label htmlFor="rule-name">规则名称</label>
          <input
            id="rule-name"
            className="input"
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="如：PDF 文档归档"
            autoFocus
          />
        </div>

        <div className="form-group">
          <label htmlFor="rule-category">目标分类</label>
          <select
            id="rule-category"
            className="input"
            style={{ width: 200 }}
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

        <div className="form-group">
          <label>匹配条件</label>
          <div className="condition-builder">
            <div className="condition-row">
              <select
                aria-label="规则类型"
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
              <input
                className="input condition-row__input"
                aria-label="匹配模式"
                value={pattern}
                onChange={(e) => setPattern(e.target.value)}
                placeholder={meta.hint}
              />
            </div>
          </div>
          <span className="hint">规则匹配优先级最高，命中后不再走启发式和 LLM。{meta.hint}</span>
        </div>

        <div className="form-group">
          <label htmlFor="rule-priority">优先级</label>
          <input
            id="rule-priority"
            className="input"
            type="number"
            style={{ maxWidth: 120 }}
            value={priority}
            onChange={(e) => setPriority(Number(e.target.value))}
          />
          <span className="hint">数字越大越先匹配（也可在列表中拖拽排序）</span>
        </div>

        <div className="setting-row" style={{ border: 'none', padding: 0 }}>
          <div className="setting-label">
            <div className="name">启用规则</div>
            <div className="desc">禁用后该规则不参与分类匹配</div>
          </div>
          <div className="setting-control">
            <button
              type="button"
              className={`toggle${isEnabled ? ' on' : ''}`}
              aria-label={isEnabled ? '禁用规则' : '启用规则'}
              aria-pressed={isEnabled}
              data-testid="rule-toggle"
              onClick={() => setIsEnabled((v) => !v)}
            />
          </div>
        </div>

        <div className="rule-form__actions">
          <button
            type="button"
            className="btn btn--primary"
            onClick={handleSubmit}
            data-testid="rule-save"
          >
            保存
          </button>
          <button
            type="button"
            className="btn btn--ghost"
            onClick={onCancel}
            data-testid="rule-cancel"
          >
            取消
          </button>
          {initial && onDelete && (
            <button
              type="button"
              className="btn btn--danger"
              onClick={handleDelete}
              data-testid="rule-delete"
            >
              删除规则
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
