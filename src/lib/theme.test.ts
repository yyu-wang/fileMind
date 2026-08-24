// theme.ts 单元测试：`<html data-theme>` 在三种模式下的应用与移除。

import { beforeEach, describe, expect, it } from 'vitest';

import { ThemeMode } from '../types/models';
import { applyTheme } from './theme';

describe('applyTheme', () => {
  beforeEach(() => {
    document.documentElement.removeAttribute('data-theme');
  });

  it('System：移除 data-theme（回落 prefers-color-scheme 媒体查询）', () => {
    document.documentElement.setAttribute('data-theme', ThemeMode.Dark);

    applyTheme(ThemeMode.System);

    expect(document.documentElement.hasAttribute('data-theme')).toBe(false);
  });

  it('Light / Dark：设置对应属性强制覆盖系统偏好', () => {
    applyTheme(ThemeMode.Light);
    expect(document.documentElement.getAttribute('data-theme')).toBe('light');

    applyTheme(ThemeMode.Dark);
    expect(document.documentElement.getAttribute('data-theme')).toBe('dark');
  });
});
