// ClassifyPreviewTree 单元测试：分组渲染、待确认高亮、冲突标记、按钮回调。

import { describe, expect, it, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';

import type { ClassifyPlanItem, ClassifyPreview } from '@/types/ipc';
import { PENDING_NAME } from '@/stores/classifyStore';
import { ClassifyPreviewTree } from './ClassifyPreviewTree';

function makeItem(id: string, overrides: Partial<ClassifyPlanItem> = {}): ClassifyPlanItem {
  return {
    file_id: id,
    file_name: `${id}.pdf`,
    original_path: `/tmp/${id}.pdf`,
    target_path: `/tmp/财务/${id}.pdf`,
    category_name: '财务',
    rule_source: 'rule:年度报表',
    status: 'Ok',
    conflict_type: null,
    ...overrides,
  };
}

function makePreview(items: ClassifyPlanItem[]): ClassifyPreview {
  const categorized = items.filter((i) => i.category_name != null).length;
  return {
    batch_id: 'b1',
    items,
    stats: {
      total: items.length,
      categorized,
      pending: items.length - categorized,
      by_rule: 1,
      by_heuristic: categorized - 1,
    },
  };
}

function renderTree(preview: ClassifyPreview) {
  const props = { preview, onExecute: vi.fn(), onReset: vi.fn() };
  render(<ClassifyPreviewTree {...props} />);
  return props;
}

describe('ClassifyPreviewTree', () => {
  it('groups items by category and shows stats', () => {
    renderTree(
      makePreview([
        makeItem('a'),
        makeItem('p', { category_name: null, rule_source: 'pending' }),
        makeItem('h', { category_name: '图片', rule_source: 'heuristic' }),
      ]),
    );

    expect(screen.getByText('财务')).toBeInTheDocument();
    expect(screen.getByText('图片')).toBeInTheDocument();
    // 待确认文案同时出现在分组头与文件项标签上
    expect(screen.getAllByText(PENDING_NAME).length).toBeGreaterThan(0);
    expect(screen.getByText('a.pdf')).toBeInTheDocument();
    expect(screen.getByText('共 3 个文件')).toBeInTheDocument();
  });

  it('marks pending group with amber class and shows pending tag', () => {
    renderTree(makePreview([makeItem('p', { category_name: null, rule_source: 'pending' })]));

    const group = document.querySelector('.classify-preview__group--pending');
    expect(group).not.toBeNull();
    expect(screen.getAllByText(PENDING_NAME).length).toBe(2);
  });

  it('renders rule source tags with correct labels', () => {
    renderTree(
      makePreview([
        makeItem('a', { rule_source: 'rule:年度报表' }),
        makeItem('h', { rule_source: 'heuristic' }),
        makeItem('p', { category_name: null, rule_source: 'pending' }),
      ]),
    );

    expect(screen.getByText('年度报表')).toBeInTheDocument();
    expect(screen.getByText('启发式')).toBeInTheDocument();
    expect(screen.getAllByText(PENDING_NAME).length).toBeGreaterThan(0);
  });

  it('shows conflict label for conflict items', () => {
    renderTree(makePreview([makeItem('c', { status: 'Conflict', conflict_type: 'SameName' })]));
    expect(screen.getByText('目标已存在（跳过）')).toBeInTheDocument();
  });

  it('calls onExecute when 开始执行 is clicked', async () => {
    const user = userEvent.setup();
    const props = renderTree(makePreview([makeItem('a')]));
    await user.click(screen.getByRole('button', { name: '开始执行' }));
    expect(props.onExecute).toHaveBeenCalledTimes(1);
  });

  it('calls onReset when 重新选择 is clicked', async () => {
    const user = userEvent.setup();
    const props = renderTree(makePreview([makeItem('a')]));
    await user.click(screen.getByRole('button', { name: '重新选择' }));
    expect(props.onReset).toHaveBeenCalledTimes(1);
  });
});
