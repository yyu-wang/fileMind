// 规则编辑页（T6.8）：规则列表 + 新建/编辑/删除/禁用 + 优先级拖拽排序。
//
// 删除采用二次确认（window.confirm）；启用开关与拖拽排序直接落库。

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

  const [formOpen, setFormOpen] = useState(false);
  const [editing, setEditing] = useState<Rule | null>(null);

  useEffect(() => {
    void load();
  }, [load]);

  const handleNew = () => {
    setEditing(null);
    setFormOpen(true);
  };

  const handleEdit = (rule: Rule) => {
    setEditing(rule);
    setFormOpen(true);
  };

  const handleSave = async (rule: Rule) => {
    try {
      await saveRule(rule);
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
    } catch {
      // 错误已由 store 记录
    }
  };

  const handleToggle = (rule: Rule) => {
    void saveRule({ ...rule, is_enabled: !rule.is_enabled }).catch(() => {
      // 错误已由 store 记录
    });
  };

  const handleReorder = (orderedIds: string[]) => {
    void reorder(orderedIds).catch(() => {
      // 错误已由 store 记录
    });
  };

  return (
    <div className="rules-page">
      <header className="rules-page__header">
        <div>
          <h1 className="rules-page__title">规则编辑</h1>
          <p className="rules-page__desc">管理分类规则：按优先级匹配，命中后归入目标分类。</p>
        </div>
        <button
          type="button"
          className="btn btn--primary"
          onClick={handleNew}
          data-testid="rules-new"
        >
          + 新建规则
        </button>
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

      <div className="rules-page__body">
        {isLoading ? (
          <LoadingState text="加载规则…" />
        ) : rules.length === 0 ? (
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
          <RuleList
            rules={rules}
            categories={categories}
            onEdit={handleEdit}
            onDelete={(rule) => void handleDelete(rule)}
            onToggle={handleToggle}
            onReorder={handleReorder}
          />
        )}
      </div>

      {formOpen && (
        <RuleForm
          initial={editing}
          categories={categories}
          onSave={(rule) => void handleSave(rule)}
          onCancel={() => setFormOpen(false)}
        />
      )}
    </div>
  );
}
