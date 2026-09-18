// 规则编辑页（对齐交互原型 §规则编辑）：左 rules-list + 右 rules-detail。
//
// 文档结构：
//   <div class="page rules-page">
//     <header class="main-header">规则编辑 + 副标题 + 新建按钮</header>
//     <div class="main-content">
//       <div class="rules-layout">
//         <RuleList/>              // 左：280px 列表 + rule-item 选中
//         <div class="rules-detail">  // 右：card + rule-form
//           <RuleForm/>            // 选中态：编辑；新建态：空表单；无选中：空态
//         </div>
//       </div>
//     </div>
//   </div>
//
// 页面只做编排：选中/表单/删除确认的状态与动作在 useRuleEditor（导入即订阅规则列表），
// 头部与主体各为一个区域组件。启用开关与拖拽排序直接落库，删除走二次确认。

import { useEffect } from 'react';

import { PageErrorBanner } from '@/components/common/PageErrorBanner';
import { LoadingState } from '@/components/common/StateViews';
import { RulesEmptyState } from '@/components/rules/RulesEmptyState';
import { RulesLayout } from '@/components/rules/RulesLayout';
import { RulesPageHeader } from '@/components/rules/RulesPageHeader';
import { ConfirmDialog } from '@/components/ui/ConfirmDialog';
import { useRuleEditor } from '@/hooks/useRuleEditor';
import { useRuleStore } from '@/stores/ruleStore';

export function RulesPage() {
  const rules = useRuleStore((s) => s.rules);
  const categories = useRuleStore((s) => s.categories);
  const isLoading = useRuleStore((s) => s.isLoading);
  const error = useRuleStore((s) => s.error);
  const load = useRuleStore((s) => s.load);
  const clearError = useRuleStore((s) => s.clearError);
  const editor = useRuleEditor(rules);
  const pendingDelete = editor.pendingDelete;

  useEffect(() => {
    void load();
  }, [load]);

  return (
    <div className="page rules-page">
      <RulesPageHeader onNew={editor.startNew} />
      <PageErrorBanner
        message={error}
        className="rules-page__error"
        dismissClassName="rules-page__error-dismiss"
        onDismiss={clearError}
      />
      <div className="main-content rules-page__body" data-testid="rules-body">
        {isLoading ? (
          <LoadingState text="加载规则…" />
        ) : rules.length === 0 && !editor.formOpen ? (
          <RulesEmptyState onNew={editor.startNew} />
        ) : (
          <RulesLayout rules={rules} categories={categories} editor={editor} />
        )}
      </div>

      {pendingDelete !== null && (
        <ConfirmDialog
          title="删除规则"
          message={`确定删除规则「${pendingDelete.name}」？此操作不可撤销。`}
          confirmLabel="删除"
          danger
          onCancel={editor.cancelDelete}
          onConfirm={editor.confirmDelete}
        />
      )}
    </div>
  );
}
