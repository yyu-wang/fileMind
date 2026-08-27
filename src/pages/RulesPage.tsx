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
// 状态：selectedId（当前编辑的规则），formOpen（点击"新建规则"时打开空表单）。
// 启用开关与拖拽排序直接落库；删除走二次确认。

import { useEffect, useState } from 'react';
import { EmptyState, LoadingState } from '../components/common/StateViews';
import { RuleForm } from '../components/rules/RuleForm';
import { RuleList } from '../components/rules/RuleList';
import { useRuleStore } from '../stores/ruleStore';
import type { Rule } from '../types/ipc';

export function RulesPage() {
  const rules = useRuleStore((s) => s.rules);
  const categories = useRuleStore((s) => s.categories);
  const isLoading = useRuleStore((s) => s.isLoading);
  const error = useRuleStore((s) => s.error);
  const load = useRuleStore((s) => s.load);
  const saveRule = useRuleStore((s) => s.saveRule);
  const deleteRule = useRuleStore((s) => s.deleteRule);
  const reorder = useRuleStore((s) => s.reorder);
  const clearError = useRuleStore((s) => s.clearError);

  // 当前选中规则 id（null + formOpen=true 表示新建态）
  const [selectedId, setSelectedId] = useState<string | null>(null);
  // 新建态：点击"+ 新建规则"时打开空表单，与编辑态互斥
  const [formOpen, setFormOpen] = useState(false);

  useEffect(() => {
    void load();
  }, [load]);

  const handleNew = () => {
    setSelectedId(null);
    setFormOpen(true);
  };

  const handleSelect = (rule: Rule) => {
    setSelectedId(rule.id);
    setFormOpen(false);
  };

  const handleSave = async (rule: Rule) => {
    try {
      await saveRule(rule);
      // 保存后选中该规则（新建态下 rule.id 在 store 中已生成）
      setSelectedId(rule.id || selectedId);
      setFormOpen(false);
    } catch {
      // 保存失败时错误已由 store 记录，保持表单打开供用户重试
    }
  };

  const handleDelete = async (rule: Rule) => {
    const confirmed = window.confirm(`确定删除规则「${rule.name}」？此操作不可撤销。`);
    if (!confirmed) return;
    try {
      await deleteRule(rule.id);
      setSelectedId(null);
      setFormOpen(false);
    } catch {
      // 错误已由 store 记录
    }
  };

  const handleReorder = (orderedIds: string[]) => {
    void reorder(orderedIds).catch(() => {
      // 错误已由 store 记录
    });
  };

  const selectedRule = selectedId ? (rules.find((r) => r.id === selectedId) ?? null) : null;
  const showForm = formOpen || selectedRule !== null;

  return (
    <div className="page rules-page">
      <header className="main-header">
        <h1>规则编辑</h1>
        <span className="subtitle">分类规则与分类体系管理</span>
        <div className="header-actions">
          <button
            type="button"
            className="btn btn--primary btn--sm"
            onClick={handleNew}
            data-testid="rules-new"
          >
            <svg
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              strokeWidth={2}
              strokeLinecap="round"
              strokeLinejoin="round"
              style={{
                width: 13,
                height: 13,
                display: 'inline-block',
                verticalAlign: '-2px',
                marginRight: 4,
              }}
              aria-hidden
            >
              <path d="M12 5v14M5 12h14" />
            </svg>
            新建规则
          </button>
        </div>
      </header>

      {error && (
        <div className="rules-page__error" role="alert">
          <span>{error}</span>
          <button
            type="button"
            className="rules-page__error-dismiss"
            aria-label="关闭错误提示"
            onClick={clearError}
          >
            ×
          </button>
        </div>
      )}

      <div className="main-content rules-page__body" data-testid="rules-body">
        {isLoading ? (
          <LoadingState text="加载规则…" />
        ) : rules.length === 0 && !formOpen ? (
          <EmptyState
            title="暂无自定义规则"
            description="内置类型识别仍会自动分类；新建规则可覆盖默认行为。"
            action={
              <button type="button" className="btn btn--primary" onClick={handleNew}>
                + 新建规则
              </button>
            }
          />
        ) : (
          <div className="rules-layout">
            <RuleList
              rules={rules}
              categories={categories}
              selectedId={selectedId}
              onSelect={handleSelect}
              onReorder={handleReorder}
            />
            <div className="rules-detail">
              {showForm ? (
                <RuleForm
                  initial={selectedRule}
                  categories={categories}
                  onSave={(rule) => void handleSave(rule)}
                  onCancel={() => {
                    setSelectedId(null);
                    setFormOpen(false);
                  }}
                  onDelete={(rule) => void handleDelete(rule)}
                />
              ) : (
                <div className="rules-empty">
                  <div className="rules-empty__icon">📋</div>
                  <div className="rules-empty__title">选择左侧规则查看详情</div>
                  <div className="rules-empty__desc">
                    点击列表中任意规则进入编辑，或点击右上角&ldquo;新建规则&rdquo;创建新规则
                  </div>
                </div>
              )}
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
