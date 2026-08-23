import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import type { FileInfo } from '@/types/ipc';
import type { SortKey } from '@/lib/fileTable';
import { FileListTable } from './FileListTable';

function makeFile(id: string, name: string, overrides: Partial<FileInfo> = {}): FileInfo {
  return {
    id,
    path: `/tmp/${name}`,
    file_name: name,
    file_size: 100,
    content_hash: null,
    category: null,
    created_at: '2026-08-01 00:00:00',
    updated_at: '2026-08-01 00:00:00',
    ...overrides,
  };
}

function renderTable(overrides: Partial<Parameters<typeof FileListTable>[0]> = {}) {
  const files = [
    makeFile('a', 'a.txt', { category: '财务' }),
    makeFile('b', 'b.md'),
    makeFile('c', 'c.png', { file_size: 2048, updated_at: '2026-08-05 10:00:00' }),
  ];
  const props = {
    files,
    selectedIds: [] as string[],
    onToggleSelect: vi.fn(),
    onSelectAll: vi.fn(),
    onOpenPreview: vi.fn(),
    sort: { key: 'name' as SortKey, dir: 'asc' as const },
    onSortChange: vi.fn(),
    ...overrides,
  };
  render(<FileListTable {...props} />);
  return props;
}

describe('FileListTable', () => {
  it('renders file rows and derived badge text', () => {
    renderTable();
    expect(screen.getByText('a.txt')).toBeInTheDocument();
    expect(screen.getByText('b.md')).toBeInTheDocument();
    expect(screen.getByText('c.png')).toBeInTheDocument();
    expect(screen.getByText('财务')).toBeInTheDocument();
    expect(screen.getAllByText('未分类')).toHaveLength(2);
    expect(screen.getByText('已分类')).toBeInTheDocument();
  });

  it('renders formatted size and time', () => {
    renderTable();
    expect(screen.getByText('2.0 KB')).toBeInTheDocument();
    expect(screen.getAllByText('100 B')).toHaveLength(2);
  });

  it('opens preview when a row is clicked', async () => {
    const user = userEvent.setup();
    const props = renderTable();
    await user.click(screen.getByText('c.png'));
    expect(props.onOpenPreview).toHaveBeenCalledTimes(1);
    expect(props.onOpenPreview).toHaveBeenCalledWith(props.files[2]);
  });

  it('toggles selection via row checkbox without opening preview', async () => {
    const user = userEvent.setup();
    const props = renderTable();
    await user.click(screen.getByLabelText('选择 a.txt'));
    expect(props.onToggleSelect).toHaveBeenCalledWith('a');
    expect(props.onOpenPreview).not.toHaveBeenCalled();
  });

  it('calls onSortChange when a header is clicked', async () => {
    const user = userEvent.setup();
    const props = renderTable();
    await user.click(screen.getByRole('button', { name: /大小/ }));
    expect(props.onSortChange).toHaveBeenCalledWith('size');
  });

  it('select-all checkbox reflects all selectable rows selected', async () => {
    const user = userEvent.setup();
    // a 已整理（软排除）：即使 a/b/c 全选，表头只按可批量项 b/c 判定
    const props = renderTable({ selectedIds: ['a', 'b', 'c'] });
    const checkbox = screen.getByLabelText('全选当前列表');
    expect(checkbox).toBeChecked();
    await user.click(checkbox);
    expect(props.onSelectAll).toHaveBeenCalledWith(null);
  });

  it('select-all checkbox calls onSelectAll with selectable ids only (excludes organized)', async () => {
    const user = userEvent.setup();
    const props = renderTable();
    // a.txt 已整理（category='财务'）→ 全选只圈选未整理的 b、c
    await user.click(screen.getByLabelText('全选当前列表'));
    expect(props.onSelectAll).toHaveBeenCalledWith(['b', 'c']);
  });

  it('select-all unselects when only organized files are selected', async () => {
    const user = userEvent.setup();
    // 只有已整理文件被选（手动勾选），表头应视为未全选 → 点击后圈选 b、c
    const props = renderTable({ selectedIds: ['a'] });
    const checkbox = screen.getByLabelText('全选当前列表');
    expect(checkbox).not.toBeChecked();
    await user.click(checkbox);
    expect(props.onSelectAll).toHaveBeenCalledWith(['b', 'c']);
  });

  it('organized rows keep manual checkbox selectable (soft exclude)', async () => {
    const user = userEvent.setup();
    const props = renderTable();
    // 已整理文件 a.txt 的勾选框仍可手动勾选（软排除 = 不进批量，但允许重分类）
    await user.click(screen.getByLabelText('选择 a.txt'));
    expect(props.onToggleSelect).toHaveBeenCalledWith('a');
  });

  it('empty list renders no rows', () => {
    renderTable({ files: [] });
    expect(screen.queryByRole('row')).not.toBeNull();
  });
});
