import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { App } from './App';
import './styles/globals.css';

const rootElement = document.getElementById('root');
if (rootElement) {
  createRoot(rootElement).render(
    <StrictMode>
      <App />
    </StrictMode>,
  );
}

// T6.1：窗口启动时 visible:false 避免白屏，React 渲染完成后调用 show 显示
// 非 Tauri 环境（纯浏览器开发调试）时调用会失败，忽略错误
getCurrentWindow()
  .show()
  .catch((e) => console.warn('[main] window.show failed (non-Tauri env?):', e));
