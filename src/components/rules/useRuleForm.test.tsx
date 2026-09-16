// useRuleForm 单元测试：草稿派生（新建 / 编辑）、字段更新、校验拦截与 payload 规范化。

import { describe, expect, it, vi } from 'vitest';
import { act, renderHook } from '@testing-library/react';

import type { Rule } from '@/types/ipc';
import { RuleType } from '@/types/models';

import { useRuleForm } from './useRuleForm';

const rule: Rule = {
  id: 'r1',
  name: 'PDF 归档',
  rule_type: RuleType.Extension,
  pattern: 'pdf',
  target_category: 'c1',
  priority: 50,
  is_enabled: false,
  created_at: '2026-01-01T00:00:00Z',
  updated_at: '2026-01-02T00:00:00Z',
};

describe('useRuleForm', () => {
  it('新建：空草稿沿用默认优先级与启用态', () => {
    const { result } = renderHook(() => useRuleForm({ initial: null, onSave: vi.fn() }));

    expect(result.current.draft).toEqual({
      name: '',
      ruleType: RuleType.Extension,
      pattern: '',
      targetCategory: '',
      priority: 100,
      isEnabled: true,
    });
    expect(result.current.error).toBeNull();
  });

  it('编辑：草稿取自待编辑规则', () => {
    const { result } = renderHook(() => useRuleForm({ initial: rule, onSave: vi.fn() }));

    expect(result.current.draft).toEqual({
      name: 'PDF 归档',
      ruleType: RuleType.Extension,
      pattern: 'pdf',
      targetCategory: 'c1',
      priority: 50,
      isEnabled: false,
    });
  });

  it('目标分类为 null 时草稿用空串', () => {
    const { result } = renderHook(() =>
      useRuleForm({ initial: { ...rule, target_category: null }, onSave: vi.fn() }),
    );

    expect(result.current.draft.targetCategory).toBe('');
  });

  it('setField 与 toggleEnabled 只改对应字段', () => {
    const { result } = renderHook(() => useRuleForm({ initial: rule, onSave: vi.fn() }));

    act(() => result.current.setField('pattern', '*.pdf'));
    act(() => result.current.toggleEnabled());

    expect(result.current.draft.pattern).toBe('*.pdf');
    expect(result.current.draft.isEnabled).toBe(true);
    expect(result.current.draft.name).toBe('PDF 归档');
  });

  it('规则名为空时拦截保存并提示', () => {
    const onSave = vi.fn();
    const { result } = renderHook(() => useRuleForm({ initial: null, onSave }));

    act(() => result.current.setField('pattern', 'pdf'));
    act(() => result.current.submit());

    expect(onSave).not.toHaveBeenCalled();
    expect(result.current.error).toBe('规则名不能为空');
  });

  it('匹配模式为空时拦截保存并提示', () => {
    const onSave = vi.fn();
    const { result } = renderHook(() => useRuleForm({ initial: null, onSave }));

    act(() => result.current.setField('name', '规则'));
    act(() => result.current.submit());

    expect(onSave).not.toHaveBeenCalled();
    expect(result.current.error).toBe('匹配模式不能为空');
  });

  it('正则类型下非法模式被前端预校验拦下', () => {
    const onSave = vi.fn();
    const { result } = renderHook(() => useRuleForm({ initial: null, onSave }));

    act(() => {
      result.current.setField('name', '规则');
      result.current.setField('ruleType', RuleType.Regex);
      result.current.setField('pattern', '(');
    });
    act(() => result.current.submit());

    expect(onSave).not.toHaveBeenCalled();
    expect(result.current.error).toBe('正则表达式不合法');
  });

  it('保存时 trim 字段、空分类转 null，并保留 id 与时间戳', () => {
    const onSave = vi.fn();
    const { result } = renderHook(() =>
      useRuleForm({ initial: { ...rule, target_category: null }, onSave }),
    );

    act(() => {
      result.current.setField('name', '  PDF 归档  ');
      result.current.setField('pattern', '  pdf  ');
      result.current.setField('targetCategory', '');
    });
    act(() => result.current.submit());

    expect(onSave).toHaveBeenCalledWith({
      id: 'r1',
      name: 'PDF 归档',
      rule_type: RuleType.Extension,
      pattern: 'pdf',
      target_category: null,
      priority: 50,
      is_enabled: false,
      created_at: '2026-01-01T00:00:00Z',
      updated_at: '2026-01-02T00:00:00Z',
    });
    expect(result.current.error).toBeNull();
  });
});
