// 左侧导航栏：5 个路由项 + 当前选中态 + 快捷键切换。
//
// 快捷键（仅侧边栏范围，全局快捷键属于 T6.10）：
//   ⌘1 文件管理  ⌘2 智能分类  ⌘3 知识问答  ⌘4 规则编辑  ⌘, 设置
//
// 图标用 inline SVG（不引入图标库，保持依赖最小；T6.4+ 若需再统一引入）

import { useEffect, type ReactNode } from 'react';
import { NavLink, useLocation, useNavigate } from 'react-router-dom';

interface NavItem {
  to: string;
  label: string;
  shortcut: string;
  icon: ReactNode;
}

const ICON_FILE = (
  <svg viewBox="0 0 16 16" width="16" height="16" aria-hidden>
    <path
      fill="currentColor"
      d="M3 1.5A1.5 1.5 0 0 1 4.5 0H9l4 4v8.5A1.5 1.5 0 0 1 11.5 14h-7A1.5 1.5 0 0 1 3 12.5v-11ZM8 1v3.5h3.5L8 1Z"
    />
  </svg>
);
const ICON_TAG = (
  <svg viewBox="0 0 16 16" width="16" height="16" aria-hidden>
    <path
      fill="currentColor"
      d="M1 2a1 1 0 0 1 1-1h6.5a1 1 0 0 1 .7.3l5.5 5.5a1 1 0 0 1 0 1.4l-6 6a1 1 0 0 1-1.4 0L1.3 8.2A1 1 0 0 1 1 7.5V2Zm3 3.5a1.5 1.5 0 1 0 0-3 1.5 1.5 0 0 0 0 3Z"
    />
  </svg>
);
const ICON_CHAT = (
  <svg viewBox="0 0 16 16" width="16" height="16" aria-hidden>
    <path
      fill="currentColor"
      d="M2 1.5C1.4 1.5 1 1.9 1 2.5v9c0 .6.4 1 1 1h2V15l3-2.5h5c.6 0 1-.4 1-1v-9c0-.6-.4-1-1-1H2Zm3 3h6v1H5v-1Zm0 3h4v1H5V7.5Z"
    />
  </svg>
);
const ICON_RULES = (
  <svg viewBox="0 0 16 16" width="16" height="16" aria-hidden>
    <path fill="currentColor" d="M2 2h12v2H2V2Zm0 3.5h8v2H2v-2Zm0 3.5h12v2H2V9Zm0 3.5h6v2H2v-2Z" />
  </svg>
);
const ICON_SETTINGS = (
  <svg viewBox="0 0 16 16" width="16" height="16" aria-hidden>
    <path
      fill="currentColor"
      d="M8 5.5a2.5 2.5 0 1 0 0 5 2.5 2.5 0 0 0 0-5Zm-6 2.5a6 6 0 0 1 .1-1L0 6l1.5-2.6 2.2.7a6 6 0 0 1 1.7-1L5.7 1h4.6l.3 2.1c.6.3 1.2.6 1.7 1l2.2-.7L16 6l-2.1 1.5a6 6 0 0 1 0 1L16 10l-1.5 2.6-2.2-.7a6 6 0 0 1-1.7 1l-.3 2.1H5.7l-.3-2.1c-.6-.3-1.2-.6-1.7-1l-2.2.7L0 10l2.1-1.5a6 6 0 0 1-.1-1Z"
    />
  </svg>
);

const NAV_ITEMS: NavItem[] = [
  { to: '/', label: '文件管理', shortcut: '1', icon: ICON_FILE },
  { to: '/classify', label: '智能分类', shortcut: '2', icon: ICON_TAG },
  { to: '/chat', label: '知识问答', shortcut: '3', icon: ICON_CHAT },
  { to: '/rules', label: '规则编辑', shortcut: '4', icon: ICON_RULES },
];

export function Sidebar() {
  const navigate = useNavigate();
  const location = useLocation();

  // 快捷键：⌘1~⌘4 切换前 4 项，⌘, 切换设置
  useEffect(() => {
    const onKeyDown = (e: globalThis.KeyboardEvent) => {
      if (!(e.metaKey || e.ctrlKey)) return;
      const map: Record<string, string> = {
        '1': '/',
        '2': '/classify',
        '3': '/chat',
        '4': '/rules',
        ',': '/settings',
      };
      const path = map[e.key];
      if (path && location.pathname !== path) {
        e.preventDefault();
        navigate(path);
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [navigate, location.pathname]);

  return (
    <nav className="sidebar" aria-label="主导航">
      <div className="sidebar__top">
        <span className="sidebar__brand">FileMind</span>
      </div>
      <ul className="sidebar__list">
        {NAV_ITEMS.map((item) => (
          <li key={item.to}>
            <NavLink
              to={item.to}
              className={({ isActive }) =>
                `sidebar__item${isActive ? ' sidebar__item--active' : ''}`
              }
              end={item.to === '/'}
              title={`${item.label}  ⌘${item.shortcut}`}
            >
              <span className="sidebar__icon">{item.icon}</span>
              <span className="sidebar__label">{item.label}</span>
            </NavLink>
          </li>
        ))}
      </ul>
      <ul className="sidebar__list sidebar__list--bottom">
        <li>
          <NavLink
            to="/settings"
            className={({ isActive }) => `sidebar__item${isActive ? ' sidebar__item--active' : ''}`}
            title="设置  ⌘,"
          >
            <span className="sidebar__icon">{ICON_SETTINGS}</span>
            <span className="sidebar__label">设置</span>
          </NavLink>
        </li>
      </ul>
    </nav>
  );
}
