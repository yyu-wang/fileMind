// 应用外框：app-frame → app-body(Sidebar + main) → statusbar。
//
// 结构（设计稿 05_交互原型 §布局）：
//   ┌─────────────────────────────────────────┐
//   │ app-frame (max 1440, 居中, box-shadow)   │
//   │ ┌────────┬───────────────────────────┐    │
//   │ │ Sidebar│   main                    │    │
//   │ │        │   ├ main-header (固定)    │    │
//   │ │        │   └ main-content (滚动)   │    │
//   │ ├────────┴───────────────────────────┤    │
//   │ │ statusbar (底部状态栏)              │    │
//   │ └─────────────────────────────────────┘    │
//   └─────────────────────────────────────────┘
//
// 注：titlebar 保留 Tauri 原生标题栏，不自绘红绿灯。
// 页面自身负责渲染 .main-header 与 .main-content，AppLayout 只提供外框。

import type { ReactNode } from 'react';
import { Sidebar } from './Sidebar';
import { StatusBar } from './StatusBar';

export interface AppLayoutProps {
  children: ReactNode;
}

export function AppLayout({ children }: AppLayoutProps) {
  return (
    <div className="app-frame">
      <div className="app-body">
        <Sidebar />
        <main className="main">{children}</main>
      </div>
      <StatusBar />
    </div>
  );
}
