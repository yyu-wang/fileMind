// 规则页主体布局：左列表 + 右详情（规则表单，或未选中时的空态）。
//
// 选中/表单/删除确认的状态与动作都在 useRuleEditor，本组件只把该 handle 分发给
// RuleList 与 RuleForm，不直接读写 store。

import type { RuleEditorHandle } from '@/hooks/useRuleEditor';
import type { Category, Rule } from '@/types/ipc';

import { RuleForm } from './RuleForm';
import { RuleList } from './RuleList';

interface RulesLayoutProps {
  /** 规则列表（左栏） */
  rules: Rule[];
  /** 目标分类下拉数据 */
  categories: Category[];
  /** 选中/表单/删除确认的状态与动作（useRuleEditor 的返回值） */
  editor: RuleEditorHandle;
}

export function RulesLayout({ rules, categories, editor }: RulesLayoutProps) {
  return (
    <div className="rules-layout">
      <RuleList
        rules={rules}
        categories={categories}
        selectedId={editor.selectedId}
        onSelect={editor.selectRule}
        onReorder={editor.reorder}
      />
      <div className="rules-detail">
        {editor.showForm ? (
          <RuleForm
            // key=选中规则 id：切换左侧规则时强制重挂载，避免表单沿用上一条规则的旧状态
            key={editor.selectedRule?.id ?? 'new-rule'}
            initial={editor.selectedRule}
            categories={categories}
            onSave={editor.save}
            onCancel={editor.cancelForm}
            onDelete={editor.requestDelete}
          />
        ) : (
          <RulesDetailEmpty />
        )}
      </div>
    </div>
  );
}

/** 右侧空态：未选中任何规则时的提示。 */
function RulesDetailEmpty() {
  return (
    <div className="rules-empty">
      <div className="rules-empty__icon">📋</div>
      <div className="rules-empty__title">选择左侧规则查看详情</div>
      <div className="rules-empty__desc">
        点击列表中任意规则进入编辑，或点击右上角&ldquo;新建规则&rdquo;创建新规则
      </div>
    </div>
  );
}
