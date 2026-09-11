// RuleForm 单元测试（对齐交互原型 §规则编辑 §rule-form）：内嵌表单、必填校验、
// 正则预校验、目标分类下拉、启用 toggle、删除按钮（仅编辑态）。

import { describe, expect, it, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import type { Category, Rule } from '@/types/ipc';
import { RuleType } from '@/types/models';

import { RuleForm } from './RuleForm';

const categories: Category[] = [
  {
    id: 'c1',
    name: '文档',
    parent_id: null,
    icon: null,
    color: null,
    sort_order: 0,
    is_builtin: true,
    target_dir: '',
    created_at: '',
    updated_at: '',
  },
];

const existingRule: Rule = {
  id: 'r1',
  name: 'PDF 归档',
  rule_type: RuleType.Extension,
  pattern: 'pdf',
  target_category: 'c1',
  priority: 10,
  is_enabled: false,
  created_at: '2026-01-01 00:00:00',
  updated_at: '2026-01-01 00:00:00',
};

describe('RuleForm', () => {
  it('renders create form with defaults', () => {
    render(<RuleForm initial={null} categories={categories} onSave={vi.fn()} onCancel={vi.fn()} />);
    expect(screen.getByText('新建规则')).toBeInTheDocument();
    expect(screen.getByLabelText('规则名称')).toHaveValue('');
    expect(screen.getByLabelText('匹配模式')).toHaveValue('');
    expect(screen.getByLabelText('优先级')).toHaveValue(100);
    expect(screen.getByText('不指定（仅打标签不移动）')).toBeInTheDocument();
    expect(screen.getByText('文档')).toBeInTheDocument();
  });

  it('renders edit form prefilled and preserves is_enabled', () => {
    render(
      <RuleForm
        initial={existingRule}
        categories={categories}
        onSave={vi.fn()}
        onCancel={vi.fn()}
      />,
    );
    expect(screen.getByText('PDF 归档')).toBeInTheDocument();
    expect(screen.getByLabelText('规则名称')).toHaveValue('PDF 归档');
    expect(screen.getByLabelText('匹配模式')).toHaveValue('pdf');
    // 编辑态：启用 toggle 处于 off（is_enabled=false）
    expect(screen.getByTestId('rule-toggle').getAttribute('aria-pressed')).toBe('false');
  });

  it('validates empty rule name', async () => {
    const user = userEvent.setup();
    const onSave = vi.fn();
    render(<RuleForm initial={null} categories={categories} onSave={onSave} onCancel={vi.fn()} />);
    await user.type(screen.getByLabelText('匹配模式'), 'pdf');
    await user.click(screen.getByRole('button', { name: '保存' }));
    expect(screen.getByRole('alert')).toHaveTextContent('规则名不能为空');
    expect(onSave).not.toHaveBeenCalled();
  });

  it('validates empty pattern', async () => {
    const user = userEvent.setup();
    const onSave = vi.fn();
    render(<RuleForm initial={null} categories={categories} onSave={onSave} onCancel={vi.fn()} />);
    await user.type(screen.getByLabelText('规则名称'), '规则');
    await user.click(screen.getByRole('button', { name: '保存' }));
    expect(screen.getByRole('alert')).toHaveTextContent('匹配模式不能为空');
    expect(onSave).not.toHaveBeenCalled();
  });

  it('rejects invalid regex pattern', async () => {
    const user = userEvent.setup();
    const onSave = vi.fn();
    render(<RuleForm initial={null} categories={categories} onSave={onSave} onCancel={vi.fn()} />);
    await user.selectOptions(screen.getByLabelText('规则类型'), RuleType.Regex);
    await user.type(screen.getByLabelText('规则名称'), '规则');
    await user.type(screen.getByLabelText('匹配模式'), '(');
    await user.click(screen.getByRole('button', { name: '保存' }));
    expect(screen.getByRole('alert')).toHaveTextContent('正则表达式不合法');
    expect(onSave).not.toHaveBeenCalled();
  });

  it('submits trimmed values with null category when unspecified', async () => {
    const user = userEvent.setup();
    const onSave = vi.fn();
    render(<RuleForm initial={null} categories={categories} onSave={onSave} onCancel={vi.fn()} />);
    await user.type(screen.getByLabelText('规则名称'), '  PDF 归档  ');
    await user.type(screen.getByLabelText('匹配模式'), '  pdf  ');
    await user.click(screen.getByRole('button', { name: '保存' }));
    expect(onSave).toHaveBeenCalledWith(
      expect.objectContaining({
        id: '',
        name: 'PDF 归档',
        rule_type: RuleType.Extension,
        pattern: 'pdf',
        target_category: null,
        priority: 100,
        is_enabled: true,
      }),
    );
  });

  it('submits selected category id when chosen', async () => {
    const user = userEvent.setup();
    const onSave = vi.fn();
    render(<RuleForm initial={null} categories={categories} onSave={onSave} onCancel={vi.fn()} />);
    await user.type(screen.getByLabelText('规则名称'), '规则');
    await user.type(screen.getByLabelText('匹配模式'), 'pdf');
    await user.selectOptions(screen.getByLabelText('目标分类'), 'c1');
    await user.click(screen.getByRole('button', { name: '保存' }));
    expect(onSave).toHaveBeenCalledWith(expect.objectContaining({ target_category: 'c1' }));
  });

  it('preserves created_at/updated_at and is_enabled on edit', async () => {
    const user = userEvent.setup();
    const onSave = vi.fn();
    render(
      <RuleForm
        initial={existingRule}
        categories={categories}
        onSave={onSave}
        onCancel={vi.fn()}
      />,
    );
    await user.click(screen.getByRole('button', { name: '保存' }));
    expect(onSave).toHaveBeenCalledWith(
      expect.objectContaining({
        id: 'r1',
        is_enabled: false,
        created_at: existingRule.created_at,
        updated_at: existingRule.updated_at,
      }),
    );
  });

  it('calls onCancel when cancel clicked', async () => {
    const user = userEvent.setup();
    const onCancel = vi.fn();
    render(
      <RuleForm initial={null} categories={categories} onSave={vi.fn()} onCancel={onCancel} />,
    );
    await user.click(screen.getByRole('button', { name: '取消' }));
    expect(onCancel).toHaveBeenCalledTimes(1);
  });

  it('hides unsupported magic_number and size rule types (P1 未开发)', () => {
    render(<RuleForm initial={null} categories={categories} onSave={vi.fn()} onCancel={vi.fn()} />);
    const select = screen.getByLabelText('规则类型');
    const option = (value: string) =>
      Array.from(select.querySelectorAll('option')).find((o) => o.value === value);
    expect(option('magic_number')).toBeUndefined();
    expect(option('size')).toBeUndefined();
    // 已支持的类型应保留
    expect(option('extension')).toBeDefined();
    expect(option('regex')).toBeDefined();
  });

  it('toggles is_enabled via toggle button', async () => {
    const user = userEvent.setup();
    const onSave = vi.fn();
    render(<RuleForm initial={null} categories={categories} onSave={onSave} onCancel={vi.fn()} />);
    // 默认 is_enabled=true，点击后翻转为 false
    const toggle = screen.getByTestId('rule-toggle');
    expect(toggle.getAttribute('aria-pressed')).toBe('true');
    await user.click(toggle);
    expect(toggle.getAttribute('aria-pressed')).toBe('false');
    await user.type(screen.getByLabelText('规则名称'), '规则');
    await user.type(screen.getByLabelText('匹配模式'), 'pdf');
    await user.click(screen.getByRole('button', { name: '保存' }));
    expect(onSave).toHaveBeenCalledWith(expect.objectContaining({ is_enabled: false }));
  });

  it('hides delete button on create mode and shows on edit mode', () => {
    const onDelete = vi.fn();
    const { rerender } = render(
      <RuleForm
        initial={null}
        categories={categories}
        onSave={vi.fn()}
        onCancel={vi.fn()}
        onDelete={onDelete}
      />,
    );
    expect(screen.queryByTestId('rule-delete')).not.toBeInTheDocument();
    rerender(
      <RuleForm
        initial={existingRule}
        categories={categories}
        onSave={vi.fn()}
        onCancel={vi.fn()}
        onDelete={onDelete}
      />,
    );
    expect(screen.getByTestId('rule-delete')).toBeInTheDocument();
  });

  it('calls onDelete when delete clicked', async () => {
    const user = userEvent.setup();
    const onDelete = vi.fn();
    render(
      <RuleForm
        initial={existingRule}
        categories={categories}
        onSave={vi.fn()}
        onCancel={vi.fn()}
        onDelete={onDelete}
      />,
    );
    await user.click(screen.getByRole('button', { name: '删除规则' }));
    expect(onDelete).toHaveBeenCalledWith(existingRule);
  });
});
