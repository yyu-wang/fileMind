// AboutSection 单元测试：手动检查更新 / 发现新版安装 / 检查失败分支。

import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';

const mocks = vi.hoisted(() => ({
  check: vi.fn(),
  relaunch: vi.fn(),
}));

vi.mock('@tauri-apps/plugin-updater', () => ({
  check: mocks.check,
}));

vi.mock('@tauri-apps/plugin-process', () => ({
  relaunch: mocks.relaunch,
}));

import { AboutSection } from './AboutSection';

describe('AboutSection 检查更新', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.relaunch.mockResolvedValue(undefined);
  });

  it('无新版本：提示已是最新', async () => {
    mocks.check.mockResolvedValue(null);

    render(<AboutSection />);
    await userEvent.click(screen.getByRole('button', { name: '检查更新' }));

    expect(await screen.findByTestId('about-update-status')).toHaveTextContent('已是最新版本');
    expect(mocks.check).toHaveBeenCalledTimes(1);
  });

  it('发现新版本：确认后下载安装并重启', async () => {
    const downloadAndInstall = vi.fn().mockResolvedValue(undefined);
    mocks.check.mockResolvedValue({
      version: '9.9.9',
      body: '修复若干问题',
      downloadAndInstall,
    });

    render(<AboutSection />);
    await userEvent.click(screen.getByRole('button', { name: '检查更新' }));

    // 确认框展示版本与说明
    expect(await screen.findByText('发现新版本')).toBeInTheDocument();
    expect(screen.getByText(/9\.9\.9/)).toBeInTheDocument();
    await userEvent.click(screen.getByRole('button', { name: '下载并安装' }));

    expect(downloadAndInstall).toHaveBeenCalledTimes(1);
    expect(mocks.relaunch).toHaveBeenCalledTimes(1);
  });

  it('检查失败：展示错误信息', async () => {
    mocks.check.mockRejectedValue(new Error('网络不可用'));

    render(<AboutSection />);
    await userEvent.click(screen.getByRole('button', { name: '检查更新' }));

    expect(await screen.findByTestId('about-update-status')).toHaveTextContent('检查更新失败');
    expect(screen.getByText(/网络不可用/)).toBeInTheDocument();
  });
});
