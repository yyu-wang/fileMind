// Sidebar 单元测试：导航项渲染、NavLink active 态、⌘1~⌘4/⌘, 快捷键跳转。

import { act, render, screen } from '@testing-library/react';
import type { ReactNode } from 'react';
import { MemoryRouter, Route, Routes, useLocation } from 'react-router-dom';
import { describe, expect, it } from 'vitest';
import { Sidebar } from './Sidebar';

/** 显示当前 pathname 的探针（验证快捷键触发的路由跳转）。 */
function LocationProbe(): ReactNode {
  const location = useLocation();
  return <span data-testid="location">{location.pathname}</span>;
}

function renderSidebarAt(path: string): ReturnType<typeof render> {
  return render(
    <MemoryRouter initialEntries={[path]}>
      <Sidebar />
      <LocationProbe />
      <Routes>
        <Route path="*" element={<span>page</span>} />
      </Routes>
    </MemoryRouter>,
  );
}

/** 派发带 ⌘ 修饰的 keydown。 */
function fireMeta(key: string): void {
  window.dispatchEvent(
    new KeyboardEvent('keydown', {
      key,
      metaKey: true,
      bubbles: true,
      cancelable: true,
    }),
  );
}

describe('Sidebar', () => {
  it('renders brand and all nav items with shortcuts', () => {
    renderSidebarAt('/');
    expect(screen.getByText('FileMind')).toBeInTheDocument();
    expect(screen.getByText('v1.0.0')).toBeInTheDocument();
    expect(screen.getAllByRole('navigation')).toHaveLength(2); // 主功能 + 系统
    for (const label of ['文件管理', '智能分类', '知识问答', '规则编辑', '设置']) {
      expect(screen.getByText(label)).toBeInTheDocument();
    }
    expect(screen.getByText('⌘1')).toBeInTheDocument();
    expect(screen.getByText('⌘,')).toBeInTheDocument();
  });

  it('marks the active route with the active class', () => {
    renderSidebarAt('/classify');
    const active = screen.getByRole('link', { name: /智能分类/ });
    expect(active).toHaveClass('active');
    expect(screen.getByRole('link', { name: /文件管理/ })).not.toHaveClass('active');
  });

  it('end prop: 文件管理 link only active on exact /', () => {
    renderSidebarAt('/classify');
    expect(screen.getByRole('link', { name: /文件管理/ })).not.toHaveClass('active');
  });

  it('⌘2 navigates to /classify', () => {
    renderSidebarAt('/');
    act(() => {
      fireMeta('2');
    });
    expect(screen.getByTestId('location')).toHaveTextContent('/classify');
  });

  it('⌘3 navigates to /chat and ⌘, to /settings', () => {
    renderSidebarAt('/chat');
    act(() => {
      fireMeta(',');
    });
    expect(screen.getByTestId('location')).toHaveTextContent('/settings');
  });

  it('same-route hotkey does not navigate (already there)', () => {
    renderSidebarAt('/chat');
    act(() => {
      fireMeta('3');
    });
    expect(screen.getByTestId('location')).toHaveTextContent('/chat');
  });
});
