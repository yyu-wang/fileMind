// 规则编辑器的本地状态（选中 / 新建表单 / 待删除）与增删改动作。
//
// 原先这些 state 与五个 handler 都堆在 RulesPage 的页面函数里（178 行）；页面只保留编排后，
// 「选中谁、表单开不开、删哪条」集中在本 Hook：selectedId 决定左侧高亮与右侧表单内容，
// formOpen 区分「新建」与「无选中」，删除走二次确认（替代阻塞式 window.confirm）。
//
// 保存 / 删除的收尾与 IPC 失败处理抽成模块级函数，避免 Hook 本体堆成一条长链。

import { useState } from 'react';

import { useRuleStore } from '@/stores/ruleStore';
import type { Rule } from '@/types/ipc';

/** 保存：成功后把表单切到编辑态（saved 含后端生成的 id），失败保持表单打开供重试。 */
async function persistRule(
  rule: Rule,
  saveRule: (rule: Rule) => Promise<Rule>,
  onSaved: (saved: Rule) => void,
): Promise<void> {
  try {
    onSaved(await saveRule(rule));
  } catch {
    // 保存失败时错误已由 store 记录，保持表单打开供用户重试
  }
}

/** 删除：成功后退出表单；失败保留当前选中（错误已由 store 记录）。 */
async function removeRule(
  id: string,
  deleteRule: (id: string) => Promise<void>,
  onRemoved: () => void,
): Promise<void> {
  try {
    await deleteRule(id);
    onRemoved();
  } catch {
    // 错误已由 store 记录
  }
}

/** 拖拽排序落库：失败由 store 记录错误。 */
function commitReorder(
  reorder: (orderedIds: string[]) => Promise<void>,
  orderedIds: string[],
): void {
  void reorder(orderedIds).catch(() => {
    // 错误已由 store 记录
  });
}

/** `useRuleEditor` 的对外出口：选中/表单/删除确认的状态与动作。 */
export interface RuleEditorHandle {
  /** 当前选中规则 id（null + formOpen=true 表示新建态） */
  selectedId: string | null;
  /** 选中规则对象（id 失配时为 null） */
  selectedRule: Rule | null;
  /** 新建态标记（点击「+ 新建规则」时打开空表单） */
  formOpen: boolean;
  /** 右侧详情是否渲染表单（新建态或已选中规则） */
  showForm: boolean;
  /** 待删除规则（ConfirmDialog 二次确认） */
  pendingDelete: Rule | null;
  /** 打开空表单（新建规则） */
  startNew: () => void;
  /** 选中某条规则进入编辑态 */
  selectRule: (rule: Rule) => void;
  /** 取消表单 / 回到无选中态 */
  cancelForm: () => void;
  /** 保存规则（成功后切到编辑态） */
  save: (rule: Rule) => void;
  /** 请求删除（打开二次确认） */
  requestDelete: (rule: Rule) => void;
  /** 取消删除 */
  cancelDelete: () => void;
  /** 确认删除 */
  confirmDelete: () => void;
  /** 拖拽排序后的最终顺序落库 */
  reorder: (orderedIds: string[]) => void;
}

/** 管理规则编辑页的选中/表单/删除确认状态与增删改动作。 */
export function useRuleEditor(rules: Rule[]): RuleEditorHandle {
  const saveRule = useRuleStore((s) => s.saveRule);
  const deleteRule = useRuleStore((s) => s.deleteRule);
  const reorderRules = useRuleStore((s) => s.reorder);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [formOpen, setFormOpen] = useState(false);
  const [pendingDelete, setPendingDelete] = useState<Rule | null>(null);

  /** 退出表单：回到「无选中」态（新建取消 / 删除成功后共用） */
  const exitForm = () => {
    setSelectedId(null);
    setFormOpen(false);
  };

  const selectRule = (rule: Rule) => {
    setSelectedId(rule.id);
    setFormOpen(false);
  };

  const save = (rule: Rule) => void persistRule(rule, saveRule, selectRule);

  const confirmDelete = () => {
    const target = pendingDelete;
    setPendingDelete(null);
    if (target !== null) void removeRule(target.id, deleteRule, exitForm);
  };

  const selectedRule =
    selectedId === null ? null : (rules.find((r) => r.id === selectedId) ?? null);

  return {
    selectedId,
    selectedRule,
    formOpen,
    showForm: formOpen || selectedRule !== null,
    pendingDelete,
    startNew: () => {
      setSelectedId(null);
      setFormOpen(true);
    },
    selectRule,
    cancelForm: exitForm,
    save,
    requestDelete: (rule: Rule) => setPendingDelete(rule),
    cancelDelete: () => setPendingDelete(null),
    confirmDelete,
    reorder: (orderedIds: string[]) => commitReorder(reorderRules, orderedIds),
  };
}
