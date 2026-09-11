// useHotkeys hook 测试：注册/清理、meta 组合（⌘/Ctrl 双适配）、
// 无 meta 按键的修饰键排除、输入框守卫、ref 同步最新 handler（防闭包过期）。

import { renderHook } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { useHotkeys, type Hotkey } from './useHotkeys';

/** 在指定元素上派发 keydown（fireEvent 之外的轻量替代：直接 dispatchEvent）。 */
function fireKey(
  target: HTMLElement | typeof window,
  key: string,
  mods: Record<string, boolean> = {},
): void {
  target.dispatchEvent(
    new KeyboardEvent('keydown', {
      key,
      metaKey: mods.meta ?? false,
      ctrlKey: mods.ctrl ?? false,
      shiftKey: mods.shift ?? false,
      altKey: mods.alt ?? false,
      bubbles: true,
      cancelable: true,
    }),
  );
}

describe('useHotkeys', () => {
  it('fires handler for plain key without modifiers', () => {
    const handler = vi.fn();
    renderHook(() => useHotkeys([{ key: 'a', handler }]));
    fireKey(window, 'a');
    expect(handler).toHaveBeenCalledTimes(1);
  });

  it('fires handler for meta combo (⌘ and Ctrl both count)', () => {
    const handler = vi.fn();
    renderHook(() => useHotkeys([{ key: '1', meta: true, handler }]));
    fireKey(window, '1', { meta: true });
    fireKey(window, '1', { ctrl: true });
    expect(handler).toHaveBeenCalledTimes(2);
  });

  it('does not fire when meta requirement mismatches', () => {
    const handler = vi.fn();
    renderHook(() => useHotkeys([{ key: '1', meta: true, handler }]));
    fireKey(window, '1'); // 缺 meta
    expect(handler).not.toHaveBeenCalled();
  });

  it('does not fire plain hotkey when shift/alt pressed', () => {
    const handler = vi.fn();
    renderHook(() => useHotkeys([{ key: 'a', handler }]));
    fireKey(window, 'a', { shift: true });
    fireKey(window, 'a', { alt: true });
    expect(handler).not.toHaveBeenCalled();
  });

  it('skips plain hotkeys inside editable targets (input guard)', () => {
    const handler = vi.fn();
    renderHook(() => useHotkeys([{ key: ' ', handler }]));
    const input = document.createElement('input');
    document.body.appendChild(input);
    try {
      fireKey(input, ' ');
      expect(handler).not.toHaveBeenCalled();
    } finally {
      input.remove();
    }
  });

  it('still fires meta hotkeys inside editable targets', () => {
    const handler = vi.fn();
    renderHook(() => useHotkeys([{ key: 'k', meta: true, handler }]));
    const input = document.createElement('input');
    document.body.appendChild(input);
    try {
      fireKey(input, 'k', { meta: true });
      expect(handler).toHaveBeenCalledTimes(1);
    } finally {
      input.remove();
    }
  });

  it('removes the window listener on unmount', () => {
    const handler = vi.fn();
    const { unmount } = renderHook(() => useHotkeys([{ key: 'a', handler }]));
    unmount();
    fireKey(window, 'a');
    expect(handler).not.toHaveBeenCalled();
  });

  it('always invokes the latest handler (no stale closure)', () => {
    const first = vi.fn();
    const second = vi.fn();
    const { rerender } = renderHook(
      ({ bindings }: { bindings: Hotkey[] }) => useHotkeys(bindings),
      { initialProps: { bindings: [{ key: 'a', handler: first }] } },
    );
    rerender({ bindings: [{ key: 'a', handler: second }] });
    fireKey(window, 'a');
    expect(first).not.toHaveBeenCalled();
    expect(second).toHaveBeenCalledTimes(1);
  });

  it('only fires the first matching binding and preventDefaults the event', () => {
    const handler = vi.fn();
    const other = vi.fn();
    renderHook(() =>
      useHotkeys([
        { key: 'a', handler },
        { key: 'b', handler: other },
      ]),
    );
    const event = new KeyboardEvent('keydown', {
      key: 'a',
      bubbles: true,
      cancelable: true,
    });
    window.dispatchEvent(event);
    expect(handler).toHaveBeenCalledTimes(1);
    expect(other).not.toHaveBeenCalled();
    expect(event.defaultPrevented).toBe(true);
  });
});
