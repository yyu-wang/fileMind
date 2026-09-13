// StatusBar 单元测试（P1-1）：模式/模型展示、文件数与选中态、引擎启动中/失败重试。
// 组件自行订阅 settings/file/sidecar store，用例通过 setState 驱动状态。

import { act, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { useFileStore } from '@/stores/fileStore';
import { useSettingsStore } from '@/stores/settingsStore';
import { useSidecarStore } from '@/stores/sidecarStore';
import type { FileStats } from '@/types/ipc';

import { StatusBar } from './StatusBar';

const emptyStats: FileStats = {
  total_files: 0,
  categorized_files: 0,
  uncategorized_files: 0,
  duplicate_groups: 0,
  total_size_bytes: 0,
};

/** 预置 settings store（本地模式 + 配置已加载基线）。 */
function seedSettings(partial: Partial<ReturnType<typeof useSettingsStore.getState>> = {}): void {
  useSettingsStore.setState({
    inferenceMode: 'Local',
    llmModel: 'qwen2.5:7b',
    cloudModel: '',
    cloudConsentProvider: null,
    isLoading: false,
    ...partial,
  });
}

/** 预置 file store（无选中、无统计基线）。 */
function seedFiles(partial: Partial<ReturnType<typeof useFileStore.getState>> = {}): void {
  useFileStore.setState({
    stats: null,
    selectedIds: [],
    clearSelection: vi.fn(),
    ...partial,
  });
}

/** 预置 sidecar store（引擎已就绪基线）。 */
function seedSidecar(partial: Partial<ReturnType<typeof useSidecarStore.getState>> = {}): void {
  useSidecarStore.setState({ status: 'ready', message: null, ...partial });
}

describe('StatusBar', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    seedSettings();
    seedFiles();
    seedSidecar();
  });

  it('展示模式标签、当前模型与版本号', () => {
    render(<StatusBar />);
    expect(screen.getByText('本地模式')).toBeInTheDocument();
    expect(screen.getByText('qwen2.5:7b')).toBeInTheDocument();
    expect(screen.getByText('v1.0.0')).toBeInTheDocument();
  });

  it('云端模式展示云端模型与对应徽标样式', () => {
    seedSettings({ inferenceMode: 'Cloud', cloudModel: 'gpt-4o-mini' });
    const { container } = render(<StatusBar />);
    expect(screen.getByText('云端模式')).toBeInTheDocument();
    expect(screen.getByText('gpt-4o-mini')).toBeInTheDocument();
    expect(container.querySelector('.mode-badge.cloud')).not.toBeNull();
  });

  it('配置加载中显示加载中，加载完成后显示文件数', () => {
    seedSettings({ isLoading: true });
    const { rerender } = render(<StatusBar />);
    expect(screen.getByText('加载中...')).toBeInTheDocument();

    // 已挂载组件后再改 store 需包 act，否则 React 报未包裹的状态更新警告
    act(() => {
      seedSettings({ isLoading: false });
      seedFiles({ stats: { ...emptyStats, total_files: 309 } });
    });
    rerender(<StatusBar />);
    expect(screen.getByText('309 文件')).toBeInTheDocument();
  });

  it('无统计数据时显示 0 文件', () => {
    render(<StatusBar />);
    expect(screen.getByText('0 文件')).toBeInTheDocument();
  });

  it('有选中文件时显示数量，点击清除调用 clearSelection', async () => {
    const user = userEvent.setup();
    const clearSelection = vi.fn();
    seedFiles({ selectedIds: ['a', 'b'], clearSelection });
    render(<StatusBar />);

    expect(screen.getByText('选中 2')).toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: '清除选中' }));
    expect(clearSelection).toHaveBeenCalledTimes(1);
  });

  it('无选中时不渲染选中区与清除按钮', () => {
    render(<StatusBar />);
    expect(screen.queryByRole('button', { name: '清除选中' })).toBeNull();
  });

  it('引擎启动中显示进行态胶囊且不提供重试', () => {
    seedSidecar({ status: 'starting' });
    render(<StatusBar />);
    expect(screen.getByText('引擎启动中…')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: '重试启动 AI 引擎' })).toBeNull();
  });

  it('引擎失败显示不可用并用错误原因作为提示，点击重试调用 retryStart', async () => {
    const user = userEvent.setup();
    const retryStart = vi.fn().mockResolvedValue(undefined);
    seedSidecar({ status: 'failed', message: '握手失败', retryStart });
    render(<StatusBar />);

    expect(screen.getByText('引擎不可用')).toBeInTheDocument();
    expect(screen.getByTitle('握手失败')).toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: '重试启动 AI 引擎' }));
    expect(retryStart).toHaveBeenCalledTimes(1);
  });

  it('崩溃循环同样视为引擎不可用', () => {
    seedSidecar({ status: 'crash_loop', message: null });
    render(<StatusBar />);
    expect(screen.getByText('引擎不可用')).toBeInTheDocument();
    // message 为空时回退到默认提示
    expect(screen.getByTitle('AI 引擎启动失败')).toBeInTheDocument();
  });
});
