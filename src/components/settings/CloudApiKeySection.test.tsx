// CloudApiKeySection 单元测试：状态回显（掩码 hint）、保存/删除流程、空输入禁用。
//
// 安全断言：完整 Key 永不渲染为文本——界面只出现 `已保存 ····abcd` 掩码，
// 输入框保存后清空，绝不保留明文。

import { beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';

vi.mock('../../lib/ipc', () => ({
  fileIpc: {
    getApiKeyStatus: vi.fn(),
    setApiKey: vi.fn(),
    deleteApiKey: vi.fn(),
  },
}));

import { fileIpc } from '../../lib/ipc';
import { useSettingsStore } from '../../stores/settingsStore';
import type { ApiKeyStatus } from '../../types/ipc';
import { CloudApiKeySection } from './CloudApiKeySection';

const NO_KEY_OPENAI: ApiKeyStatus = { provider: 'Openai', has_key: false, hint: '' };
const NO_KEY_DEEPSEEK: ApiKeyStatus = { provider: 'Deepseek', has_key: false, hint: '' };
const CONFIGURED_OPENAI: ApiKeyStatus = { provider: 'Openai', has_key: true, hint: '····abcd' };
const CONFIGURED_DEEPSEEK: ApiKeyStatus = { provider: 'Deepseek', has_key: true, hint: '····wxyz' };

/** 初始无 Key 状态 + 加载状态 mock（两服务商均未配置）。 */
function mockNoKeys() {
  vi.mocked(fileIpc.getApiKeyStatus).mockResolvedValue({
    status: 'ok',
    data: [NO_KEY_OPENAI, NO_KEY_DEEPSEEK],
  });
}

beforeEach(() => {
  vi.clearAllMocks();
  useSettingsStore.setState({
    apiKeyStatus: { Openai: NO_KEY_OPENAI, Deepseek: NO_KEY_DEEPSEEK },
    error: null,
  });
});

function openaiRow(): HTMLElement {
  const input = screen.getByLabelText('OpenAI API Key 输入框');
  const row = input.closest('.setting-row');
  // .setting-row 恒为 div，转 HTMLElement 供 within 使用
  return row as HTMLElement;
}

describe('CloudApiKeySection', () => {
  it('renders both providers with 未配置 badge when no keys stored', async () => {
    mockNoKeys();
    render(<CloudApiKeySection />);

    expect(screen.getByText(/OpenAI API Key/)).toBeInTheDocument();
    expect(screen.getByText(/DeepSeek API Key/)).toBeInTheDocument();
    await waitFor(() => expect(screen.getAllByText('未配置')).toHaveLength(2));
  });

  it('shows masked hint instead of full key for configured providers', async () => {
    vi.mocked(fileIpc.getApiKeyStatus).mockResolvedValue({
      status: 'ok',
      data: [CONFIGURED_OPENAI, CONFIGURED_DEEPSEEK],
    });
    render(<CloudApiKeySection />);

    await screen.findByText('已保存 ····abcd');
    expect(screen.getByText('已保存 ····wxyz')).toBeInTheDocument();
    // 完整 Key 永不渲染为文本
    expect(screen.queryByText(/sk-proj/)).not.toBeInTheDocument();
  });

  it('saves a new key, clears the input and shows masked status', async () => {
    const user = userEvent.setup();
    mockNoKeys();
    vi.mocked(fileIpc.setApiKey).mockResolvedValue({ status: 'ok', data: CONFIGURED_OPENAI });
    render(<CloudApiKeySection />);
    await waitFor(() => expect(screen.getAllByText('未配置')).toHaveLength(2));

    const input = screen.getByLabelText('OpenAI API Key 输入框');
    await user.type(input, 'sk-proj-abcdefghijklmnop');
    await user.click(within(openaiRow()).getByRole('button', { name: '保存' }));

    expect(vi.mocked(fileIpc.setApiKey)).toHaveBeenCalledWith('Openai', 'sk-proj-abcdefghijklmnop');
    await screen.findByText('已保存 ····abcd');
    // 输入框清空，明文不驻留
    expect(input).toHaveValue('');
  });

  it('deletes a key and shows 未配置 again', async () => {
    const user = userEvent.setup();
    // FE-m13：删除前有 window.confirm 二次确认，需 mock 放行
    vi.spyOn(window, 'confirm').mockReturnValue(true);
    vi.mocked(fileIpc.getApiKeyStatus).mockResolvedValue({
      status: 'ok',
      data: [CONFIGURED_OPENAI, NO_KEY_DEEPSEEK],
    });
    vi.mocked(fileIpc.deleteApiKey).mockResolvedValue({ status: 'ok', data: NO_KEY_OPENAI });
    render(<CloudApiKeySection />);
    await screen.findByText('已保存 ····abcd');

    await user.click(within(openaiRow()).getByRole('button', { name: '删除' }));

    expect(vi.mocked(fileIpc.deleteApiKey)).toHaveBeenCalledWith('Openai');
    await waitFor(() => expect(screen.getAllByText('未配置')).toHaveLength(2));
  });

  it('disables save button while input is empty', async () => {
    mockNoKeys();
    render(<CloudApiKeySection />);

    expect(within(openaiRow()).getByRole('button', { name: '保存' })).toBeDisabled();
  });

  it('surfaces save errors in the section error area', async () => {
    const user = userEvent.setup();
    mockNoKeys();
    vi.mocked(fileIpc.setApiKey).mockResolvedValue({
      status: 'error',
      error: 'KEY-100:API Key 不能为空',
    });
    render(<CloudApiKeySection />);
    await waitFor(() => expect(screen.getAllByText('未配置')).toHaveLength(2));

    const input = screen.getByLabelText('OpenAI API Key 输入框');
    await user.type(input, 'sk-proj-abcdefghijklmnop');
    await user.click(within(openaiRow()).getByRole('button', { name: '保存' }));

    expect(screen.getByRole('alert')).toHaveTextContent('KEY-100:API Key 不能为空');
  });
});
