// RuleEnabledToggle 单元测试：开/关态的类名与无障碍属性、点击回调。

import { describe, expect, it, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';

import { RuleEnabledToggle } from './RuleEnabledToggle';

describe('RuleEnabledToggle', () => {
  it('关闭态：无 on 类，按钮语义为「启用规则」', () => {
    render(<RuleEnabledToggle enabled={false} onToggle={vi.fn()} />);

    const toggle = screen.getByTestId('rule-toggle');
    expect(toggle).not.toHaveClass('on');
    expect(toggle).toHaveAttribute('aria-label', '启用规则');
    expect(toggle).toHaveAttribute('aria-pressed', 'false');
  });

  it('开启态：带 on 类，按钮语义为「禁用规则」', () => {
    render(<RuleEnabledToggle enabled onToggle={vi.fn()} />);

    const toggle = screen.getByTestId('rule-toggle');
    expect(toggle).toHaveClass('toggle', 'on');
    expect(toggle).toHaveAttribute('aria-label', '禁用规则');
    expect(toggle).toHaveAttribute('aria-pressed', 'true');
  });

  it('渲染标签与说明，点击上报切换意图', async () => {
    const user = userEvent.setup();
    const onToggle = vi.fn();
    render(<RuleEnabledToggle enabled={false} onToggle={onToggle} />);

    expect(screen.getByText('启用规则')).toBeInTheDocument();
    expect(screen.getByText('禁用后该规则不参与分类匹配')).toBeInTheDocument();

    await user.click(screen.getByTestId('rule-toggle'));
    expect(onToggle).toHaveBeenCalledTimes(1);
  });
});
