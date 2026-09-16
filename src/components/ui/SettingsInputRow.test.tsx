// SettingsInputRow 单元测试：标签/说明/输入框渲染、错误提示与标红、回调透传。

import { describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';

import { SettingsInputRow } from './SettingsInputRow';

function renderRow(overrides: Partial<Parameters<typeof SettingsInputRow>[0]> = {}) {
  const props = {
    label: '请求地址 Base URL',
    desc: '不含尾斜杠',
    value: '',
    onChange: vi.fn(),
    ...overrides,
  };
  render(<SettingsInputRow {...props} />);
  return props;
}

describe('SettingsInputRow', () => {
  it('渲染标签、说明与输入框', () => {
    renderRow({ placeholder: 'https://example.com/v1', ariaLabel: 'Base URL' });

    expect(screen.getByText('请求地址 Base URL')).toBeInTheDocument();
    expect(screen.getByText('不含尾斜杠')).toBeInTheDocument();
    expect(screen.getByLabelText('Base URL')).toHaveAttribute(
      'placeholder',
      'https://example.com/v1',
    );
  });

  it('无 desc 时不渲染说明行', () => {
    const { container } = render(<SettingsInputRow label="备注" value="" onChange={vi.fn()} />);
    expect(container.querySelector('.desc')).toBeNull();
  });

  it('有错误时给出提示并给输入框加错误类', () => {
    renderRow({ error: '请求地址不能为空', ariaLabel: 'Base URL' });

    expect(screen.getByRole('alert')).toHaveTextContent('请求地址不能为空');
    expect(screen.getByLabelText('Base URL')).toHaveClass('input--error');
  });

  it('无错误时不渲染提示，输入框只有基础类', () => {
    renderRow({ error: null, ariaLabel: 'Base URL' });

    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
    expect(screen.getByLabelText('Base URL')).toHaveClass('input');
    expect(screen.getByLabelText('Base URL')).not.toHaveClass('input--error');
  });

  it('输入与失焦分别上报，且变更只上报值', async () => {
    const user = userEvent.setup();
    const props = renderRow({ ariaLabel: 'Base URL', onBlur: vi.fn() });
    const input = screen.getByLabelText('Base URL');

    fireEvent.change(input, { target: { value: 'https://example.com/v1' } });
    expect(props.onChange).toHaveBeenLastCalledWith('https://example.com/v1');

    input.focus();
    await user.tab();
    expect(props.onBlur).toHaveBeenCalledTimes(1);
  });

  it('disabled 透传；last 去掉行底边框', () => {
    const { container } = render(
      <SettingsInputRow label="备注" value="x" onChange={vi.fn()} disabled last ariaLabel="备注" />,
    );

    expect(screen.getByLabelText('备注')).toBeDisabled();
    const row = container.querySelector('.setting-row');
    expect(row?.getAttribute('style')).toContain('border-bottom');
  });

  it('非末行不带内联样式', () => {
    const { container } = render(
      <SettingsInputRow label="备注" value="x" onChange={vi.fn()} ariaLabel="备注" />,
    );
    expect(container.querySelector('.setting-row')?.getAttribute('style')).toBeNull();
  });
});
