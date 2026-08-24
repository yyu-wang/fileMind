// 三栏布局外壳：Sidebar（左侧）+ 主内容区（右上）+ StatusBar（右下）。
//
// CSS Grid：
//   ┌──────────┬───────────────────┐
//   │ Sidebar  │   Main content    │
//   │ (200px)  │   (flex 1)        │
//   │          ├───────────────────┤
//   │          │   StatusBar (28px)│
//   └──────────┴───────────────────┘
//
// 响应式：窗口宽度 < 900px 时侧边栏收为 48px 图标条（设计稿 9.1）

import type { ReactNode } from 'react';
import { Sidebar } from './Sidebar';
import { StatusBar } from './StatusBar';

export interface AppLayoutProps {
  children: ReactNode;
}

export function AppLayout({ children }: AppLayoutProps) {
  return (
    <div className="app-layout">
      <Sidebar />
      <main className="app-layout__main">{children}</main>
      <StatusBar />
    </div>
  );
}
