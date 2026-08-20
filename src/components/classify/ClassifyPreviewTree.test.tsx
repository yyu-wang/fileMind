// ClassifyPreviewTree 单元测试：树形分组、待确认高亮、冲突标记、按钮回调。

import { describe, expect, it } from 'vitest';
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
      by_heuristic: Math.max(categorized - 1, 0),
    },
  };
}

function renderTree(preview: ClassifyPreview) {
  render(<ClassifyPreviewTree preview={preview} />);
}

describe('ClassifyPreviewTree', () => {
  it('groups items into tree panels by category and shows header count', () => {
    renderTree(
      makePreview([
        makeItem('a'),
        makeItem('p', { category_name: null, rule_source: 'pending' }),
        makeItem('h', { category_name: '图片', rule_source: 'heuristic' }),
      ]),
    );

    expect(screen.getByText('财务')).toBeInTheDocument();
    expect(screen.getByText('图片')).toBeInTheDocument();
    // 待确认组头「❓ 待确认」
    expect(screen.getByText(`❓ ${PENDING_NAME}`)).toBeInTheDocument();
    expect(screen.getByText('a.pdf')).toBeInTheDocument();
    expect(screen.getByText('2 个已分类 · 1 个待确认')).toBeInTheDocument();
  });

  it('marks pending group with confirm panel class and pending tag', () => {
    renderTree(makePreview([makeItem('p', { category_name: null, rule_source: 'pending' })]));

    const group = document.querySelector('.tree-panel.confirm');
    expect(group).not.toBeNull();
    // 文件项来源 tag 显示「待确认」
    expect(screen.getByText(PENDING_NAME)).toBeInTheDocument();
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
    expect(screen.getByText('按类型')).toBeInTheDocument();
    expect(screen.getByText(PENDING_NAME)).toBeInTheDocument();
  });

  it('shows conflict label for conflict items in dedicated conflict panel', () => {
    renderTree(makePreview([makeItem('c', { status: 'Conflict', conflict_type: 'SameName' })]));

    expect(screen.getByText('⚠️ 冲突')).toBeInTheDocument();
    expect(screen.getByText('目标已存在（跳过）')).toBeInTheDocument();
  });

  it('collapses and expands a tree panel on parent click', async () => {
    const user = userEvent.setup();
    renderTree(makePreview([makeItem('a')]));

    // 默认展开：文件可见
    expect(screen.getByText('a.pdf')).toBeInTheDocument();
    await user.click(screen.getByText('财务'));
    // 折叠后文件隐藏
    expect(screen.queryByText('a.pdf')).not.toBeInTheDocument();
    await user.click(screen.getByText('财务'));
    expect(screen.getByText('a.pdf')).toBeInTheDocument();
  });
});
