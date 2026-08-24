// 应用内快捷键 hook（T6.10 键盘快捷键系统）。
//
// 统一管理 window keydown 监听，声明式注册快捷键：
//   useHotkeys([
//     { key: '1', meta: true, handler: () => navigate('/') },
//     { key: ' ', handler: () => preview() },
//   ]);
//
// 设计要点：
// - useEffect 空依赖只注册一次监听；最新 bindings 经 ref 在每次渲染后同步，
//   事件处理始终读取最新 handler（避免闭包过期）
// - meta 修饰键：macOS 用 ⌘（e.metaKey），Windows/Linux 用 Ctrl（e.ctrlKey）
// - 输入框守卫：无 meta 修饰的快捷键（如 Space）在 input/textarea/select 内跳过，
//   避免「输入空格触发预览」；带 meta 的组合键不跳过（⌘ 组合不用于文本输入）

import { useEffect, useRef } from 'react';

export interface Hotkey {
  /** 主键（`e.key`），如 '1'、','、'n'、' ' */
  key: string;
  /** 是否需要 meta（⌘/Ctrl）修饰，默认 false */
  meta?: boolean;
  /** 匹配后触发（已在内部 preventDefault） */
  handler: () => void;
}

/** 焦点是否落在可编辑元素上（input/textarea/select/contentEditable）。 */
function isEditableTarget(target: globalThis.EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  const tag = target.tagName;
  return tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT' || target.isContentEditable;
}

/** 判断按键事件是否命中单个快捷键绑定。 */
function matches(hotkey: Hotkey, e: globalThis.KeyboardEvent): boolean {
  const metaPressed = e.metaKey || e.ctrlKey;
  if ((hotkey.meta ?? false) !== metaPressed) return false;
  if (hotkey.meta) {
    // meta 组合键：仅要求 meta + 主键匹配（⌘N、⌘, 等）
    return e.key === hotkey.key;
  }
  // 无 meta 的快捷键：要求无任何修饰键（纯按键，如 Space）
  if (e.shiftKey || e.altKey) return false;
  return e.key === hotkey.key;
}

/**
 * 注册应用内快捷键。
 *
 * bindings 数组每次渲染可安全内联创建（内部用 ref 保存最新引用）。
 */
export function useHotkeys(hotkeys: Hotkey[]): void {
  const ref = useRef(hotkeys);

  // 每次渲染后同步最新 bindings（事件处理读取 ref.current，始终为最新）
  useEffect(() => {
    ref.current = hotkeys;
  });

  useEffect(() => {
    const onKeyDown = (e: globalThis.KeyboardEvent) => {
      const binding = ref.current.find((h) => matches(h, e));
      if (!binding) return;

      // 输入框守卫：无 meta 的快捷键在可编辑元素内跳过
      if (!binding.meta && isEditableTarget(e.target)) return;

      e.preventDefault();
      binding.handler();
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, []);
}
