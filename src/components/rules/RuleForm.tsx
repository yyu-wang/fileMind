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
//       <div class="setting-row">…启用规则…</div>
//       <div class="rule-form__actions">保存 / 取消 / 删除</div>
//     </div>
//   </div>
//
// 后端约束：rule_type 仅支持 extension / path_keyword / regex；magic_number / size
// P1 未开发，已从条件类型下拉中隐藏（不渲染禁用占位）。多条件构造（AND/OR）需要后端
// 支持，当前仅渲染单 condition-row 与 rule_type/pattern 一对一映射。
//
// 表单草稿与校验收在 useRuleForm，启用开关是独立的 RuleEnabledToggle。

import type { Category, Rule } from '../../types/ipc';
import { RULE_TYPE_META, type RuleType } from '../../types/models';

import { RuleEnabledToggle } from './RuleEnabledToggle';
import { useRuleForm } from './useRuleForm';

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

const RULE_TYPE_OPTIONS = (Object.keys(RULE_TYPE_META) as RuleType[])
  .filter((value) => !RULE_TYPE_META[value].disabled)
  .map((value) => ({
    value,
    label: RULE_TYPE_META[value].label,
  }));

/** 标题：编辑态优先显示规则名，其次「编辑规则」；新建为「新建规则」。 */
function formTitle(initial: Rule | null): string {
  if (!initial) {
    return '新建规则';
  }
  return initial.name || '编辑规则';
}

export function RuleForm({ initial, categories, onSave, onCancel, onDelete }: RuleFormProps) {
  const { draft, error, setField, toggleEnabled, submit } = useRuleForm({ initial, onSave });
  const meta = RULE_TYPE_META[draft.ruleType];

  const handleDelete = () => {
    if (initial && onDelete) {
      onDelete(initial);
    }
  };

  return (
    <div className="card" data-testid="rule-form">
      <h3>{formTitle(initial)}</h3>

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
            value={draft.name}
            onChange={(e) => setField('name', e.target.value)}
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
            value={draft.targetCategory}
            onChange={(e) => setField('targetCategory', e.target.value)}
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
                value={draft.ruleType}
                onChange={(e) => setField('ruleType', e.target.value as RuleType)}
              >
                {RULE_TYPE_OPTIONS.map((opt) => (
                  <option key={opt.value} value={opt.value}>
                    {opt.label}
                  </option>
                ))}
              </select>
              <input
                className="input condition-row__input"
                aria-label="匹配模式"
                value={draft.pattern}
                onChange={(e) => setField('pattern', e.target.value)}
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
            value={draft.priority}
            onChange={(e) => setField('priority', Number(e.target.value))}
          />
          <span className="hint">数字越大越先匹配（也可在列表中拖拽排序）</span>
        </div>

        <RuleEnabledToggle enabled={draft.isEnabled} onToggle={toggleEnabled} />

        <div className="rule-form__actions">
          <button
            type="button"
            className="btn btn--primary"
            onClick={submit}
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
