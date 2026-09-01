// Toast 通知系统测试：store 增删、自动关闭时序、容器渲染、action 按钮交互。
//
// 时序：show → duration 后进入 removing- 状态（250ms 动画）→ 真正移除。
// duration=0 不自动关闭；action 按钮点击后回调 + 手动移除。

import { beforeEach, describe, expect, it, vi } from 'vitest';
import { act, fireEvent, render, screen } from '@testing-library/react';
import { ToastContainer, useToastStore } from './Toast';

beforeEach(() => {
  vi.clearAllMocks();
  useToastStore.setState({ toasts: [] });
});

describe('useToastStore.show', () => {
  it('adds a toast and returns its id', () => {
    const id = useToastStore.getState().show({ message: '保存成功', variant: 'success' });
    expect(id).toMatch(/^toast-\d+$/);
    expect(useToastStore.getState().toasts).toHaveLength(1);
    expect(useToastStore.getState().toasts[0].message).toBe('保存成功');
  });

  it('appends multiple toasts', () => {
    act(() => {
      useToastStore.getState().show({ message: '第一条', variant: 'info' });
      useToastStore.getState().show({ message: '第二条', variant: 'warn' });
    });
    expect(useToastStore.getState().toasts).toHaveLength(2);
  });
});

describe('ToastContainer', () => {
  it('renders nothing when there are no toasts', () => {
    const { container } = render(<ToastContainer />);
    expect(container.firstChild).toBeNull();
  });

  it('renders toast message with variant icon', () => {
    act(() => {
      useToastStore.getState().show({ message: '保存成功', variant: 'success', duration: 0 });
    });
    render(<ToastContainer />);
    const alert = screen.getByRole('alert');
    expect(alert).toHaveTextContent('保存成功');
    expect(screen.getByText('✓')).toBeInTheDocument();
  });

  it('renders error variant with ✗ icon', () => {
    act(() => {
      useToastStore.getState().show({ message: '出错了', variant: 'error', duration: 0 });
    });
    render(<ToastContainer />);
    expect(screen.getByText('✗')).toBeInTheDocument();
  });

  it('auto removes after duration (removing → gone)', () => {
    vi.useFakeTimers();
    try {
      act(() => {
        useToastStore.getState().show({ message: '自动关闭', variant: 'info', duration: 500 });
      });
      const { container } = render(<ToastContainer />);
      expect(screen.getByRole('alert')).toBeInTheDocument();

      act(() => {
        vi.advanceTimersByTime(500);
      });
      // 进入离场动画阶段（removing 类）
      expect(screen.getByRole('alert')).toHaveClass('removing');

      act(() => {
        vi.advanceTimersByTime(250);
      });
      expect(container.firstChild).toBeNull();
    } finally {
      vi.useRealTimers();
    }
  });

  it('duration=0 keeps the toast until manual remove', () => {
    vi.useFakeTimers();
    try {
      act(() => {
        useToastStore.getState().show({ message: '不自动关', variant: 'warn', duration: 0 });
      });
      render(<ToastContainer />);
      act(() => {
        vi.advanceTimersByTime(10_000);
      });
      expect(screen.getByRole('alert')).toBeInTheDocument();
    } finally {
      vi.useRealTimers();
    }
  });

  it('action button fires onAction then removes the toast', () => {
    vi.useFakeTimers();
    try {
      const onAction = vi.fn();
      act(() => {
        useToastStore.getState().show({
          message: '有新版本',
          variant: 'info',
          actionLabel: '查看',
          onAction,
          duration: 0,
        });
      });
      render(<ToastContainer />);
      fireEvent.click(screen.getByRole('button', { name: '查看' }));
      expect(onAction).toHaveBeenCalledTimes(1);

      // 手动移除 → removing 动画 → 消失
      act(() => {
        vi.advanceTimersByTime(250);
      });
      expect(screen.queryByRole('alert')).not.toBeInTheDocument();
    } finally {
      vi.useRealTimers();
    }
  });
});
