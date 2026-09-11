// sidecarStore 单元测试（P1-1）：状态事件分发 / 启动查询 / 手动重试 / 订阅幂等。

import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
}));

vi.mock('../types/ipc', () => ({
  commands: {
    getSidecarStatus: vi.fn(),
    retrySidecarStart: vi.fn(),
  },
}));

import { listen } from '@tauri-apps/api/event';
import { commands } from '../types/ipc';
import { useSidecarStore } from './sidecarStore';

describe('sidecarStore', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    // 重置瞬态，避免用例间状态残留
    useSidecarStore.setState({ status: 'starting', message: null, restartCount: 0 });
  });

  it('处理 ready 事件', () => {
    useSidecarStore.getState().handleStatusEvent({ status: 'ready', message: null });
    expect(useSidecarStore.getState().status).toBe('ready');
    expect(useSidecarStore.getState().message).toBeNull();
  });

  it('处理 failed 事件并携带原因', () => {
    useSidecarStore.getState().handleStatusEvent({ status: 'failed', message: '握手失败' });
    expect(useSidecarStore.getState().status).toBe('failed');
    expect(useSidecarStore.getState().message).toBe('握手失败');
  });

  it('未知状态兜底 crash_loop（排障友好）', () => {
    useSidecarStore.getState().handleStatusEvent({ status: 'crashed', message: 'x' });
    expect(useSidecarStore.getState().status).toBe('crash_loop');
  });

  it('refreshStatus 查询真值并更新重启计数', async () => {
    vi.mocked(commands.getSidecarStatus).mockResolvedValue({
      status: 'ok',
      data: { status: 'failed', message: 'boom', restart_count: 3 },
    });
    await useSidecarStore.getState().refreshStatus();
    const state = useSidecarStore.getState();
    expect(state.status).toBe('failed');
    expect(state.message).toBe('boom');
    expect(state.restartCount).toBe(3);
  });

  it('refreshStatus IPC 异常仅告警不抛错', async () => {
    vi.mocked(commands.getSidecarStatus).mockRejectedValue(new Error('ipc down'));
    await expect(useSidecarStore.getState().refreshStatus()).resolves.toBeUndefined();
  });

  it('retryStart 成功后乐观置 starting', async () => {
    vi.mocked(commands.retrySidecarStart).mockResolvedValue({ status: 'ok', data: null });
    await useSidecarStore.getState().retryStart();
    expect(useSidecarStore.getState().status).toBe('starting');
  });

  it('retryStart 被拒时记录错误信息', async () => {
    vi.mocked(commands.retrySidecarStart).mockResolvedValue({
      status: 'error',
      error: 'Sidecar 正在启动中，请稍候',
    });
    await useSidecarStore.getState().retryStart();
    expect(useSidecarStore.getState().message).toBe('Sidecar 正在启动中，请稍候');
  });

  it('initListener 幂等注册一次', async () => {
    await useSidecarStore.getState().initListener();
    await useSidecarStore.getState().initListener();
    expect(vi.mocked(listen)).toHaveBeenCalledTimes(1);
    expect(vi.mocked(listen)).toHaveBeenCalledWith('sidecar-status', expect.any(Function));
  });
});
