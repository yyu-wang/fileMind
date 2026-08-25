// App 组件测试（FE-M11）：启动配置加载失败 → 错误卡片 + 重试入口。
//
// 此前 loadConfig 异常（typedError 对 Error rethrow）时 isLoading 永久 true，
// App 停在「正在加载配置...」白屏且无任何恢复路径。

import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen } from '@testing-library/react';

import { useSettingsStore } from '@/stores/settingsStore';
import { App } from './App';

// App 会渲染路由页面（FilesPage 等），它们依赖 fileIpc——统一 mock 掉
vi.mock('@/lib/ipc', () => ({
  fileIpc: {
    scanDirectory: vi.fn(),
    listAllFiles: vi.fn(),
    getFileStats: vi.fn(),
  },
}));

// FilePreviewDrawer 顶层 import react-pdf → pdf.js 在 jsdom 崩（DOMMatrix 未定义）。
// 本测试只关心 App 的加载分支，PDF 渲染整体 mock 掉。
vi.mock('react-pdf', () => ({
  Document: () => null,
  Page: () => null,
  pdfjs: { GlobalWorkerOptions: { workerSrc: '' } },
}));

describe('App（FE-M11 启动失败恢复）', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    localStorage.clear();
  });

  it('initFailed=true：渲染错误卡片而非白屏/引导流', () => {
    useSettingsStore.setState({ isLoading: false, initFailed: true, error: 'IPC 断连' });
    render(<App />);

    const alert = screen.getByRole('alert');
    expect(alert).toHaveTextContent('配置加载失败');
    expect(alert).toHaveTextContent('IPC 断连');
    // 关键：不能误入引导向导（加载失败 ≠ 未完成引导）
    expect(screen.queryByText(/欢迎|引导|开始使用/)).not.toBeInTheDocument();
  });

  it('点击重试调用 retryInit', () => {
    const loadConfig = vi.fn().mockResolvedValue(undefined);
    useSettingsStore.setState({ isLoading: false, initFailed: true, error: 'IPC 断连' });
    // retryInit 内部调 loadConfig；直接替换 store action 验证接线
    useSettingsStore.setState({ retryInit: async () => loadConfig() });

    render(<App />);
    fireEvent.click(screen.getByTestId('app-retry-init'));

    expect(loadConfig).toHaveBeenCalledTimes(1);
  });

  it('正常路径回归：isLoading 时仍显示加载态', () => {
    useSettingsStore.setState({ isLoading: true, initFailed: false });
    render(<App />);
    expect(screen.getByText('正在加载配置...')).toBeInTheDocument();
  });
});
