// RuleList 单元测试：渲染、分类兜底、启用切换、编辑/删除与拖拽重排。

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
    onEdit: vi.fn(),
    onDelete: vi.fn(),
    onToggle: vi.fn(),
    onReorder: vi.fn(),
    disabledIds: [],
    ...overrides,
  };
  render(<RuleList {...props} />);
  return props;
}

describe('RuleList', () => {
  it('renders rule name, type label, pattern, category and priority', () => {
    renderList();
    expect(screen.getByText('规则 r1')).toBeInTheDocument();
    expect(screen.getAllByText('扩展名')).toHaveLength(2);
    expect(screen.getByText('pdf')).toBeInTheDocument();
    expect(screen.getByText('文档')).toBeInTheDocument();
    expect(screen.getByText('#20')).toBeInTheDocument();
  });

  it('renders disabled tag when rule is disabled', () => {
    renderList({ rules: [rule('r1', { is_enabled: false })] });
    expect(screen.getByText('已禁用')).toBeInTheDocument();
  });

  it('maps missing category to 已删除分类 and null to em dash', () => {
    renderList({
      rules: [rule('r1', { target_category: 'gone' }), rule('r2', { target_category: null })],
    });
    expect(screen.getByText('已删除分类')).toBeInTheDocument();
    expect(screen.getByText('—')).toBeInTheDocument();
  });

  it('toggles rule via checkbox with aria-label', async () => {
    const props = renderList();
    fireEvent.click(screen.getByLabelText('禁用规则 规则 r1'));
    expect(props.onToggle).toHaveBeenCalledWith(props.rules[0]);
  });

  it('calls onEdit and onDelete from buttons', () => {
    const props = renderList();
    fireEvent.click(screen.getAllByRole('button', { name: '编辑' })[0]);
    expect(props.onEdit).toHaveBeenCalledWith(props.rules[0]);
    fireEvent.click(screen.getAllByRole('button', { name: '删除' })[0]);
    expect(props.onDelete).toHaveBeenCalledWith(props.rules[0]);
  });

  it('reorders on drop and submits final id order', () => {
    const props = renderList();
    const items = screen.getAllByRole('listitem');
    fireEvent.dragStart(items[0]);
    fireEvent.drop(items[1]);
    // r1 拖到 r2 之后 → [r2, r1]
    expect(props.onReorder).toHaveBeenCalledWith(['r2', 'r1']);
  });

  it('drop on same item does not reorder', () => {
    const props = renderList();
    const items = screen.getAllByRole('listitem');
    fireEvent.dragStart(items[0]);
    fireEvent.drop(items[0]);
    expect(props.onReorder).not.toHaveBeenCalled();
  });

  it('adds dragging class to the dragged item', () => {
    renderList();
    const items = screen.getAllByRole('listitem');
    fireEvent.dragStart(items[0]);
    expect(items[0].className).toContain('rules-item--dragging');
    fireEvent.dragEnd(items[0]);
    expect(items[0].className).not.toContain('rules-item--dragging');
  });
});
