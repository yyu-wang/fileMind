import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { App } from './App';
import { applyTheme } from './lib/theme';
import { useChatStore } from './stores/chatStore';
import { useFileStore } from './stores/fileStore';
import { useSettingsStore } from './stores/settingsStore';
import './styles/globals.css';

// T6.9：渲染前应用持久化的主题偏好（避免首帧闪烁）
// zustand persist 对 localStorage 同步 rehydrate，getState().theme 已是持久化值
applyTheme(useSettingsStore.getState().theme);

const rootElement = document.getElementById('root');
if (rootElement) {
  createRoot(rootElement).render(
    <StrictMode>
      <App />
    </StrictMode>,
  );
}

// T6.2：启动时从 Rust 端加载配置与文件统计，确保 StatusBar 显示真实数据
// 不阻塞渲染（fire-and-forget），加载完成 store 自动触发 UI 更新
Promise.all([useSettingsStore.getState().loadConfig(), useFileStore.getState().loadStats()]).catch(
  (e) => console.warn('[main] loadConfig/loadStats failed:', e),
);

// T6.6：订阅 chat://event（Rust 代理 Sidecar SSE 帧）；非 Tauri 环境监听失败仅告警
useChatStore
  .getState()
  .initChatListener()
  .catch((e) => console.warn('[main] chat listener init failed:', e));

// T6.1：窗口启动时 visible:false 避免白屏，React 渲染完成后调用 show 显示
// 非 Tauri 环境（纯浏览器开发调试）时调用会失败，忽略错误
getCurrentWindow()
  .show()
  .catch((e) => console.warn('[main] window.show failed (non-Tauri env?):', e));
