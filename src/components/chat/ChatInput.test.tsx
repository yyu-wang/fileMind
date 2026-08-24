// ChatInput 单元测试：Enter 发送、Shift+Enter 换行、禁用态、空输入不发。

import { describe, expect, it, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';

import { ChatInput } from './ChatInput';

describe('ChatInput', () => {
  it('renders textarea and send button', () => {
    render(<ChatInput disabled={false} onSend={vi.fn()} />);
    expect(screen.getByLabelText('问题输入框')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: '发送' })).toBeInTheDocument();
  });

  it('sends trimmed value and clears on send button', async () => {
    const user = userEvent.setup();
    const onSend = vi.fn();
    render(<ChatInput disabled={false} onSend={onSend} />);
    const input = screen.getByLabelText('问题输入框');
    await user.type(input, '  你好  ');
    await user.click(screen.getByRole('button', { name: '发送' }));
    expect(onSend).toHaveBeenCalledWith('你好');
    expect(input).toHaveValue('');
  });

  it('sends on Enter key', async () => {
    const user = userEvent.setup();
    const onSend = vi.fn();
    render(<ChatInput disabled={false} onSend={onSend} />);
    await user.type(screen.getByLabelText('问题输入框'), '查询{enter}');
    expect(onSend).toHaveBeenCalledWith('查询');
  });

  it('Shift+Enter inserts newline without sending', async () => {
    const user = userEvent.setup();
    const onSend = vi.fn();
    render(<ChatInput disabled={false} onSend={onSend} />);
    await user.type(screen.getByLabelText('问题输入框'), 'a{shift>}{enter}{/shift}b');
    expect(onSend).not.toHaveBeenCalled();
  });

  it('empty or whitespace input does not send and disables button', async () => {
    const user = userEvent.setup();
    const onSend = vi.fn();
    render(<ChatInput disabled={false} onSend={onSend} />);
    expect(screen.getByRole('button', { name: '发送' })).toBeDisabled();
    await user.type(screen.getByLabelText('问题输入框'), '   ');
    expect(screen.getByRole('button', { name: '发送' })).toBeDisabled();
    await user.click(screen.getByRole('button', { name: '发送' }));
    expect(onSend).not.toHaveBeenCalled();
  });

  it('disables both field and button when disabled', () => {
    render(<ChatInput disabled onSend={vi.fn()} />);
    expect(screen.getByLabelText('问题输入框')).toBeDisabled();
    expect(screen.getByRole('button', { name: '发送' })).toBeDisabled();
  });
});
