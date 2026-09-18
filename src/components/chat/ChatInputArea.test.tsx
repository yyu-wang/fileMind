// ChatInputArea 单元测试：引擎状态提示、输入框禁用态与两个回调。

import { describe, expect, it, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';

import { ChatInputArea } from './ChatInputArea';

const PLACEHOLDER = '输入问题，Enter 发送，Shift+Enter 换行';

function renderArea(overrides: Partial<Parameters<typeof ChatInputArea>[0]> = {}) {
  const props = {
    engineReady: true,
    engineFailed: false,
    streaming: false,
    onRetryEngine: vi.fn(),
    onSend: vi.fn(),
    ...overrides,
  };
  render(<ChatInputArea {...props} />);
  return props;
}

describe('ChatInputArea', () => {
  it('引擎就绪时不显示提示且输入可用', () => {
    renderArea();
    expect(screen.queryByRole('status')).not.toBeInTheDocument();
    expect(screen.getByPlaceholderText(PLACEHOLDER)).toBeEnabled();
  });

  it('引擎启动中显示提示并禁用输入', () => {
    renderArea({ engineReady: false });
    expect(screen.getByRole('status')).toHaveTextContent('AI 引擎启动中，就绪后可开始问答…');
    expect(screen.getByPlaceholderText(PLACEHOLDER)).toBeDisabled();
  });

  it('引擎失败时给出重试入口', async () => {
    const user = userEvent.setup();
    const props = renderArea({ engineReady: false, engineFailed: true });

    expect(screen.getByRole('status')).toHaveTextContent('AI 引擎启动失败，暂时无法提问。');
    await user.click(screen.getByRole('button', { name: '重试' }));
    expect(props.onRetryEngine).toHaveBeenCalledTimes(1);
  });

  it('流式输出期间禁用输入', () => {
    renderArea({ streaming: true });
    expect(screen.getByPlaceholderText(PLACEHOLDER)).toBeDisabled();
  });

  it('回车发送并把内容上报给父级', async () => {
    const user = userEvent.setup();
    const props = renderArea();

    await user.type(screen.getByPlaceholderText(PLACEHOLDER), '什么是 RAG{Enter}');

    expect(props.onSend).toHaveBeenCalledWith('什么是 RAG');
  });
});
