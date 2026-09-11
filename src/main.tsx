import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { App } from './App';
import { ErrorBoundary } from './components/common/ErrorBoundary';
import { applyTheme } from './lib/theme';
import { useChatStore } from './stores/chatStore';
import { useFileStore } from './stores/fileStore';
import { useSettingsStore } from './stores/settingsStore';
import { useSidecarStore } from './stores/sidecarStore';
import './styles/globals.css';

// T9.5：@wdio/tauri-service 在 macOS 上依赖 `window.__wdio_original_core__` 做每个命令的
// window-state focus-check。但 tauri-plugin-wdio-webdriver 1.3.0 在 macOS 走原生 DirectEval
// 通道、从不注入该 guest-js 全局 → 每次 focus-check 干等 5s 后失败，E2E 全部超时
// （spike 只是恰好卡在预算内）。这里把 Tauri v2 的 `__TAURI_INTERNALS__`（含 `.invoke`）
// 别名为该全局，让 focus-check 毫秒级解析。生产无副作用：本就是这个语义别名。
declare global {
  interface Window {
    /** @wdio/tauri-service 的 macOS core 别名（见上方 shim 注释） */
    __wdio_original_core__?:
      | {
          invoke: (cmd: string, args?: unknown) => Promise<unknown>;
        }
      | undefined;
  }
}

const tauriInternals = (
  window as unknown as {
    __TAURI_INTERNALS__?: {
      invoke: (cmd: string, args?: unknown) => Promise<unknown>;
    };
  }
).__TAURI_INTERNALS__;
window.__wdio_original_core__ ||= tauriInternals;

// T6.9：渲染前应用持久化的主题偏好（避免首帧闪烁）
// zustand persist 对 localStorage 同步 rehydrate，getState().theme 已是持久化值
applyTheme(useSettingsStore.getState().theme);

const rootElement = document.getElementById('root');
if (rootElement) {
  // P1-2：React 挂载前移除 index.html 内联启动 splash。
  // 在同一同步任务内完成「移除 + 首次 render」，期间无绘制，不会闪白；
  // 若 JS 加载失败（splash 未被移除），splash 保留可见而非白屏。
  rootElement.querySelector('#fm-splash')?.remove();
  createRoot(rootElement).render(
    <StrictMode>
      <ErrorBoundary>
        <App />
      </ErrorBoundary>
    </StrictMode>,
  );
}

// T6.2：启动时从 Rust 端加载配置与文件统计，确保 StatusBar 显示真实数据
// 不阻塞渲染（fire-and-forget），加载完成 store 自动触发 UI 更新
// FE-M11：allSettled——loadConfig 失败已由 store 落 initFailed（App 渲染重试卡片），
// Promise.all 会因首个 reject 跳过 loadStats；settled 保证两者都执行
Promise.allSettled([
  useSettingsStore.getState().loadConfig(),
  useFileStore.getState().loadStats(),
]).then((results) => {
  for (const r of results) {
    if (r.status === 'rejected') console.warn('[main] init task failed:', r.reason);
  }
});

// T6.6：订阅 chat://event（Rust 代理 Sidecar SSE 帧）；非 Tauri 环境监听失败仅告警
useChatStore
  .getState()
  .initChatListener()
  .catch((e) => console.warn('[main] chat listener init failed:', e));

// P1-1：订阅 sidecar-status 事件 + 查询初始状态（引擎状态胶囊 / AI 功能门控）。
// 事件可能在页面监听建立前就发出（窗口秒开后侧车仍在启动），refreshStatus 兜底对齐。
useSidecarStore
  .getState()
  .initListener()
  .catch((e) => console.warn('[main] sidecar listener init failed:', e));
void useSidecarStore.getState().refreshStatus();

// T6.1：窗口启动时 visible:false 避免白屏，React 渲染完成后调用 show 显示
// 非 Tauri 环境（纯浏览器开发调试）时调用会失败，忽略错误
getCurrentWindow()
  .show()
  .catch((e) => console.warn('[main] window.show failed (non-Tauri env?):', e));
