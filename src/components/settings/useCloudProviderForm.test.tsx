// useCloudProviderForm 单元测试：初值派生、touched 门控错误展示、提交链路与 payload 规范化。

import { describe, expect, it, vi } from 'vitest';
import { act, renderHook } from '@testing-library/react';
import type { FormEvent } from 'react';

import type { CloudProviderRecord } from '@/types/ipc';

import { useCloudProviderForm } from './useCloudProviderForm';

const record: CloudProviderRecord = {
  id: '1',
  provider_key: 'qwen',
  name: 'DashScope',
  remark: '公司',
  website: 'https://dashscope.aliyun.com',
  base_url: 'https://dashscope.aliyuncs.com/compatible-mode/v1',
  is_builtin: false,
  created_at: '2026-01-01T00:00:00Z',
  updated_at: '2026-01-01T00:00:00Z',
};

/** 提交事件桩：被测逻辑只用到 preventDefault。 */
function submitEvent(): FormEvent<HTMLElement> {
  return { preventDefault: vi.fn() } as unknown as FormEvent<HTMLElement>;
}

describe('useCloudProviderForm', () => {
  it('新建模式：初值全空，未触摸时不展示错误', () => {
    const { result } = renderHook(() => useCloudProviderForm({ initial: null, onSubmit: vi.fn() }));

    expect(result.current.isEdit).toBe(false);
    expect(result.current.values).toEqual({
      slug: '',
      name: '',
      remark: '',
      website: '',
      baseUrl: '',
    });
    expect(result.current.errorFor('name')).toBeNull();
  });

  it('编辑模式：初值取自记录，且不校验 slug', () => {
    const { result } = renderHook(() =>
      useCloudProviderForm({ initial: record, onSubmit: vi.fn() }),
    );

    expect(result.current.isEdit).toBe(true);
    expect(result.current.values).toEqual({
      slug: 'qwen',
      name: 'DashScope',
      remark: '公司',
      website: 'https://dashscope.aliyun.com',
      baseUrl: 'https://dashscope.aliyuncs.com/compatible-mode/v1',
    });
    act(() => result.current.touch('slug'));
    expect(result.current.errorFor('slug')).toBeNull();
  });

  it('错误文案在触摸该字段后才对外可见', () => {
    const { result } = renderHook(() => useCloudProviderForm({ initial: null, onSubmit: vi.fn() }));

    expect(result.current.errorFor('name')).toBeNull();
    act(() => result.current.touch('name'));
    expect(result.current.errorFor('name')).toBe('显示名称不能为空');
    expect(result.current.errorFor('remark')).toBeNull();
  });

  it('校验不通过时只提示，不调用 onSubmit', async () => {
    const onSubmit = vi.fn();
    const { result } = renderHook(() => useCloudProviderForm({ initial: null, onSubmit }));

    await act(async () => {
      await result.current.handleSubmit(submitEvent());
    });

    expect(onSubmit).not.toHaveBeenCalled();
    expect(result.current.formError).toBe('请修正下方表单错误');
  });

  it('校验通过时提交规范化 payload（空备注/官网转 null）', async () => {
    const onSubmit = vi.fn().mockResolvedValue(undefined);
    const { result } = renderHook(() => useCloudProviderForm({ initial: null, onSubmit }));

    act(() => {
      result.current.setField('slug', 'qwen');
      result.current.setField('name', 'DashScope');
      result.current.setField('baseUrl', 'https://example.com/v1');
    });
    await act(async () => {
      await result.current.handleSubmit(submitEvent());
    });

    expect(onSubmit).toHaveBeenCalledWith({
      provider_key: 'qwen',
      name: 'DashScope',
      remark: null,
      website: null,
      base_url: 'https://example.com/v1',
    });
    expect(result.current.formError).toBeNull();
  });

  it('提交抛错时把消息落到表单错误', async () => {
    const onSubmit = vi.fn().mockRejectedValue(new Error('保存失败：标识重复'));
    const { result } = renderHook(() => useCloudProviderForm({ initial: record, onSubmit }));

    await act(async () => {
      await result.current.handleSubmit(submitEvent());
    });

    expect(result.current.formError).toBe('保存失败：标识重复');
  });

  it('提交抛出的非 Error 用兜底文案', async () => {
    const onSubmit = vi.fn().mockRejectedValue('boom');
    const { result } = renderHook(() => useCloudProviderForm({ initial: record, onSubmit }));

    await act(async () => {
      await result.current.handleSubmit(submitEvent());
    });

    expect(result.current.formError).toBe('保存提供商失败');
  });
});
