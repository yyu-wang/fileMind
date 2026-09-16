// 规则表单的草稿状态与提交：字段值、校验与保存 payload 组装。
//
// 从 RuleForm 抽出——组件的圈复杂度此前为 20，其中 12 个分支来自六处
// `initial?.x ?? 默认值`；改成「无 initial 直接返回空草稿」后只剩 1 个分支，
// 校验与 payload 组装也各自成为可单测的小函数。

import { useState } from 'react';

import type { Rule } from '@/types/ipc';
import { RuleType } from '@/types/models';

/** 表单草稿：字段与保存 payload 同形，但都是未规范化的原始输入。 */
export interface RuleDraft {
  name: string;
  ruleType: RuleType;
  pattern: string;
  targetCategory: string;
  priority: number;
  isEnabled: boolean;
}

/** 可编辑字段名。 */
export type RuleDraftField = keyof RuleDraft;

/** 新建规则的初值（与后端默认一致：优先级 100、默认启用）。 */
const EMPTY_DRAFT: RuleDraft = {
  name: '',
  ruleType: RuleType.Extension,
  pattern: '',
  targetCategory: '',
  priority: 100,
  isEnabled: true,
};

/** 由待编辑规则派生草稿；新建时返回空草稿的副本（不与模块常量共享引用）。 */
function initialDraft(initial: Rule | null): RuleDraft {
  if (!initial) {
    return { ...EMPTY_DRAFT };
  }
  return {
    name: initial.name,
    ruleType: initial.rule_type as RuleType,
    pattern: initial.pattern,
    targetCategory: initial.target_category ?? '',
    priority: initial.priority,
    isEnabled: initial.is_enabled,
  };
}

/** 校验草稿，返回首条错误文案；通过时返回 null。 */
function validateRuleDraft(draft: RuleDraft): string | null {
  if (!draft.name.trim()) {
    return '规则名不能为空';
  }
  if (!draft.pattern.trim()) {
    return '匹配模式不能为空';
  }
  if (draft.ruleType === RuleType.Regex) {
    // 前端预校验正则合法性，避免后端执行期才报错
    try {
      RegExp(draft.pattern);
    } catch {
      return '正则表达式不合法';
    }
  }
  return null;
}

/** 草稿 → 保存 payload：trim、空分类转 null、编辑态保留 id 与时间戳。 */
function toRule(draft: RuleDraft, initial: Rule | null): Rule {
  return {
    id: initial?.id ?? '',
    name: draft.name.trim(),
    rule_type: draft.ruleType,
    pattern: draft.pattern.trim(),
    target_category: draft.targetCategory || null,
    priority: draft.priority,
    is_enabled: draft.isEnabled,
    created_at: initial?.created_at ?? '',
    updated_at: initial?.updated_at ?? '',
  };
}

/** useRuleForm 的入参。 */
export interface UseRuleFormOptions {
  /** 待编辑的规则（null 表示新建） */
  initial: Rule | null;
  /** 校验通过后的保存回调 */
  onSave: (rule: Rule) => void;
}

/** useRuleForm 的对外出口。 */
export interface RuleFormHandle {
  /** 当前草稿 */
  draft: RuleDraft;
  /** 校验错误文案（通过或未提交时为 null） */
  error: string | null;
  /** 更新单个字段 */
  setField: <K extends RuleDraftField>(field: K, value: RuleDraft[K]) => void;
  /** 切换启用状态 */
  toggleEnabled: () => void;
  /** 提交：校验不通过时只展示错误，不调用 onSave */
  submit: () => void;
}

/**
 * 管理规则表单的草稿状态与提交。
 *
 * Args:
 *   options: 待编辑规则与保存回调（见 UseRuleFormOptions）
 *
 * Returns:
 *   草稿、错误文案与字段更新/提交动作（见 RuleFormHandle）
 */
export function useRuleForm({ initial, onSave }: UseRuleFormOptions): RuleFormHandle {
  const [draft, setDraft] = useState<RuleDraft>(() => initialDraft(initial));
  const [error, setError] = useState<string | null>(null);

  const setField = <K extends RuleDraftField>(field: K, value: RuleDraft[K]) => {
    setDraft((prev) => ({ ...prev, [field]: value }));
  };

  const toggleEnabled = () => {
    setDraft((prev) => ({ ...prev, isEnabled: !prev.isEnabled }));
  };

  const submit = () => {
    const message = validateRuleDraft(draft);
    if (message !== null) {
      setError(message);
      return;
    }
    setError(null);
    onSave(toRule(draft, initial));
  };

  return { draft, error, setField, toggleEnabled, submit };
}
