// RuleList 单元测试（对齐交互原型 §规则编辑）：rule-item 渲染、选中态、tags、拖拽重排。

import { describe, expect, it, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import type { Category, Rule } from '@/types/ipc';
import { RuleType } from '@/types/models';

import { RuleList } from './RuleList';

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

function rule(id: string, overrides: Partial<Rule> = {}): Rule {
  return {
    id,
    name: `规则 ${id}`,
    rule_type: RuleType.Extension,
    pattern: 'pdf',
    target_category: 'c1',
    priority: 10,
    is_enabled: true,
    created_at: '',
    updated_at: '',
    ...overrides,
  };
}

function renderList(overrides: Partial<Parameters<typeof RuleList>[0]> = {}) {
  const props = {
    rules: [rule('r1', { priority: 20 }), rule('r2', { pattern: 'doc', target_category: null })],
    categories,
    selectedId: null,
    onSelect: vi.fn(),
    onReorder: vi.fn(),
    ...overrides,
  };
  render(<RuleList {...props} />);
  return props;
}

describe('RuleList', () => {
  it('renders rule name, description, type tag and priority', () => {
    renderList();
    expect(screen.getByText('规则 r1')).toBeInTheDocument();
    expect(screen.getByText('#20')).toBeInTheDocument();
    // 启用 tag（r2 因 target_category=null 仍展示启用 tag，因为 is_enabled=true）
    expect(screen.getAllByText('启用')).toHaveLength(2);
    // 类型 tag（两条规则都是 extension）
    expect(screen.getAllByText('扩展名')).toHaveLength(2);
  });

  it('renders disabled tag when rule is disabled', () => {
    renderList({ rules: [rule('r1', { is_enabled: false })] });
    expect(screen.getByText('已禁用')).toBeInTheDocument();
  });

  it('renders count tag in header', () => {
    renderList({ rules: [rule('r1')] });
    expect(screen.getByText('1 条')).toBeInTheDocument();
  });

  it('calls onSelect when rule-item clicked', () => {
    const props = renderList();
    const items = screen.getAllByRole('button');
    fireEvent.click(items[0]);
    expect(props.onSelect).toHaveBeenCalledWith(props.rules[0]);
  });

  it('applies active class to selected rule', () => {
    renderList({ selectedId: 'r1' });
    const items = screen.getAllByTestId('rule-item');
    expect(items[0].className).toContain('active');
    expect(items[1].className).not.toContain('active');
  });

  it('reorders on drop and submits final id order', () => {
    const props = renderList();
    const items = screen.getAllByTestId('rule-item');
    fireEvent.dragStart(items[0]);
    fireEvent.drop(items[1]);
    // r1 拖到 r2 之后 → [r2, r1]
    expect(props.onReorder).toHaveBeenCalledWith(['r2', 'r1']);
  });

  it('drop on same item does not reorder', () => {
    const props = renderList();
    const items = screen.getAllByTestId('rule-item');
    fireEvent.dragStart(items[0]);
    fireEvent.drop(items[0]);
    expect(props.onReorder).not.toHaveBeenCalled();
  });

  it('adds dragging class to the dragged item', () => {
    renderList();
    const items = screen.getAllByTestId('rule-item');
    fireEvent.dragStart(items[0]);
    expect(items[0].className).toContain('rule-item--dragging');
    fireEvent.dragEnd(items[0]);
    expect(items[0].className).not.toContain('rule-item--dragging');
  });
});
