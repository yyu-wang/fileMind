// 主题应用工具（T6.9 暗色模式）。
//
// 通过 `<html data-theme="light|dark">` 控制主题；「跟随系统」时移除属性，
// 让 globals.css 的 `prefers-color-scheme` 媒体查询纯 CSS 响应系统切换，
// 无需 JS 监听 matchMedia。

import { ThemeMode } from '../types/models';

/**
 * 把主题模式应用到 `<html>` 的 `data-theme` 属性。
 *
 * - `System`：移除 `data-theme`，回落到媒体查询（跟随系统）
 * - `Light` / `Dark`：设置对应属性，强制覆盖系统偏好
 */
export function applyTheme(mode: ThemeMode): void {
  const root = document.documentElement;
  if (mode === ThemeMode.System) {
    root.removeAttribute('data-theme');
  } else {
    root.setAttribute('data-theme', mode);
  }
}
