// 左侧导航栏（设计稿 05_交互原型 §Sidebar）。
//
// 结构：sidebar-brand(logo + name + ver) → nav-label「主功能」→ nav-item ×4
//       → nav-spacer → nav-label「系统」→ nav-item(设置)
//
// 快捷键（仅侧边栏范围，全局快捷键属于 T6.10）：
//   ⌘1 文件管理  ⌘2 智能分类  ⌘3 知识问答  ⌘4 规则编辑  ⌘, 设置
//
// 图标用 inline SVG（24×24 stroke 风格，对齐文档）

import { type ReactNode } from 'react';
import { NavLink, useLocation, useNavigate } from 'react-router-dom';
import { useHotkeys } from '../../hooks/useHotkeys';

interface NavItem {
  to: string;
  label: string;
  shortcut: string;
  icon: ReactNode;
}

// 24×24 stroke 风格图标（与设计文档一致）
const ICON_FILE = (
  <svg
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth={2}
    strokeLinecap="round"
    strokeLinejoin="round"
    aria-hidden
  >
    <path d="M3 7v10a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2V9a2 2 0 0 0-2-2h-6l-2-2H5a2 2 0 0 0-2 2z" />
  </svg>
);
const ICON_TAG = (
  <svg
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth={2}
    strokeLinecap="round"
    strokeLinejoin="round"
    aria-hidden
  >
    <path d="m12 3-1.9 5.8a2 2 0 0 1-1.3 1.3L3 12l5.8 1.9a2 2 0 0 1 1.3 1.3L12 21l1.9-5.8a2 2 0 0 1 1.3-1.3L21 12l-5.8-1.9a2 2 0 0 1-1.3-1.3z" />
  </svg>
);
const ICON_CHAT = (
  <svg
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth={2}
    strokeLinecap="round"
    strokeLinejoin="round"
    aria-hidden
  >
    <path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z" />
  </svg>
);
const ICON_RULES = (
  <svg
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth={2}
    strokeLinecap="round"
    strokeLinejoin="round"
    aria-hidden
  >
    <path d="M20 7h-9M14 17H5M20 7a3 3 0 0 1-3 3H8a3 3 0 0 1-3-3M20 7a3 3 0 0 0-3-3H8a3 3 0 0 0-3 3" />
  </svg>
);
const ICON_SETTINGS = (
  <svg
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth={2}
    strokeLinecap="round"
    strokeLinejoin="round"
    aria-hidden
  >
    <circle cx="12" cy="12" r="3" />
    <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1 0 2.83 2 2 0 0 1-2.83 0l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-2 2 2 2 0 0 1-2-2v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83 0 2 2 0 0 1 0-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1-2-2 2 2 0 0 1 2-2h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 0-2.83 2 2 0 0 1 2.83 0l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 2-2 2 2 0 0 1 2 2v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 0 2 2 0 0 1 0 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 2 2 2 2 0 0 1-2 2h-.09a1.65 1.65 0 0 0-1.51 1z" />
  </svg>
);
const LOGO_ICON = (
  <svg
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth={2}
    strokeLinecap="round"
    strokeLinejoin="round"
    aria-hidden
  >
    <path d="M3 7v10a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2V9a2 2 0 0 0-2-2h-6l-2-2H5a2 2 0 0 0-2 2z" />
  </svg>
);

const MAIN_NAV: NavItem[] = [
  { to: '/', label: '文件管理', shortcut: '⌘1', icon: ICON_FILE },
  { to: '/classify', label: '智能分类', shortcut: '⌘2', icon: ICON_TAG },
  { to: '/chat', label: '知识问答', shortcut: '⌘3', icon: ICON_CHAT },
  { to: '/rules', label: '规则编辑', shortcut: '⌘4', icon: ICON_RULES },
];

const SYSTEM_NAV: NavItem[] = [
  { to: '/settings', label: '设置', shortcut: '⌘,', icon: ICON_SETTINGS },
];

export function Sidebar() {
  const navigate = useNavigate();
  const location = useLocation();

  // T6.10 快捷键：⌘1~⌘4 切换前 4 项，⌘, 切换设置
  const navigateTo = (path: string) => {
    if (location.pathname !== path) navigate(path);
  };

  useHotkeys([
    { key: '1', meta: true, handler: () => navigateTo('/') },
    { key: '2', meta: true, handler: () => navigateTo('/classify') },
    { key: '3', meta: true, handler: () => navigateTo('/chat') },
    { key: '4', meta: true, handler: () => navigateTo('/rules') },
    { key: ',', meta: true, handler: () => navigateTo('/settings') },
  ]);

  return (
    <aside className="sidebar" aria-label="主导航">
      <div className="sidebar-brand">
        <div className="logo">{LOGO_ICON}</div>
        <div>
          <div className="name">FileMind</div>
          <div className="ver">v1.0.0</div>
        </div>
      </div>

      <div className="nav-label">主功能</div>
      <nav className="nav-list">
        {MAIN_NAV.map((item) => (
          <NavLink
            key={item.to}
            to={item.to}
            className={({ isActive }) => `nav-item${isActive ? ' active' : ''}`}
            end={item.to === '/'}
            title={`${item.label}  ${item.shortcut}`}
          >
            {item.icon}
            <span>{item.label}</span>
            <span className="shortcut">{item.shortcut}</span>
          </NavLink>
        ))}
      </nav>

      <div className="nav-spacer" />

      <div className="nav-label">系统</div>
      <nav className="nav-list">
        {SYSTEM_NAV.map((item) => (
          <NavLink
            key={item.to}
            to={item.to}
            className={({ isActive }) => `nav-item${isActive ? ' active' : ''}`}
            title={`${item.label}  ${item.shortcut}`}
          >
            {item.icon}
            <span>{item.label}</span>
            <span className="shortcut">{item.shortcut}</span>
          </NavLink>
        ))}
      </nav>
    </aside>
  );
}
