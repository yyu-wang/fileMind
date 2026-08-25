// ClassifyPage 单元测试：初始态按钮文案（无文件/未整理/全部分类）、历史视图与错误提示。

import { beforeEach, describe, expect, it, vi } from 'vitest';
import { act, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import type { FileInfo } from '@/types/ipc';
import { ClassifyStatus } from '@/types/models';

import { useClassifyHistoryStore } from '@/stores/classifyHistoryStore';
import { useClassifyStore } from '@/stores/classifyStore';
import { useFileStore } from '@/stores/fileStore';
import { ClassifyPage } from './ClassifyPage';

const mocks = vi.hoisted(() => ({
  readFilePreview: vi.fn(),
  classifyPreview: vi.fn(),
  listCategories: vi.fn(),
  executeOperations: vi.fn(),
  updateFileCategory: vi.fn(),
  undoBatch: vi.fn(),
  getOperationHistory: vi.fn(),
  getBatchDetail: vi.fn(),
  Document: vi.fn((props: { children?: unknown }) => props.children),
  Page: vi.fn(() => null),
}));

vi.mock('@/lib/ipc', () => ({
  fileIpc: {
    readFilePreview: mocks.readFilePreview,
    classifyPreview: mocks.classifyPreview,
    listCategories: mocks.listCategories,
    executeOperations: mocks.executeOperations,
    updateFileCategory: mocks.updateFileCategory,
    undoBatch: mocks.undoBatch,
    getOperationHistory: mocks.getOperationHistory,
    getBatchDetail: mocks.getBatchDetail,
  },
}));

vi.mock('react-pdf', () => ({
  pdfjs: { GlobalWorkerOptions: {} },
  Document: mocks.Document,
  Page: mocks.Page,
}));

function file(overrides: Partial<FileInfo> = {}): FileInfo {
  return {
    id: 'f1',
    path: '/tmp/a.pdf',
    file_name: 'a.pdf',
    file_size: 100,
    content_hash: null,
    category: null,
    created_at: '2026-08-01 00:00:00',
    updated_at: '2026-08-01 00:00:00',
    ...overrides,
  };
}

beforeEach(() => {
  localStorage.clear();
  vi.clearAllMocks();
  useFileStore.setState({ files: [], scanPath: null, selectedIds: [] });
  useClassifyStore.setState({
    status: ClassifyStatus.Idle,
    preview: null,
    progress: { done: 0, total: 0 },
    execSummary: null,
    lastBatchId: null,
    error: null,
  });
  useClassifyHistoryStore.setState({
    batches: [],
    detail: null,
    loading: false,
    undoing: false,
    error: null,
  });
  mocks.getOperationHistory.mockResolvedValue({
    status: 'ok',
    data: { batches: [], page: 1, page_size: 20 },
  });
});

function renderPage(): void {
  render(<ClassifyPage />);
}

describe('ClassifyPage', () => {
  it('shows intro and disabled start button when no files', () => {
    renderPage();
    expect(screen.getByText('按规则与文件类型自动整理')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: '请先在文件页扫描目录' })).toBeDisabled();
  });

  it('shows 全部分类 enabled when files present', () => {
    useFileStore.setState({ files: [file()], scanPath: '/tmp' });
    renderPage();
    const start = screen.getByRole('button', { name: '全部分类' });
    expect(start).toBeEnabled();
  });

  it('shows 没有未整理的文件 disabled when all files organized', () => {
    useFileStore.setState({
      files: [file({ category: '文档' })],
      scanPath: '/tmp',
    });
    renderPage();
    const start = screen.getByRole('button', { name: '没有未整理的文件' });
    expect(start).toBeDisabled();
  });

  it('shows selection-specific label when files selected', () => {
    // 选中态在挂载后注入：挂载时 selectedIds 为空避免触发自动预览
    useFileStore.setState({ files: [file()], scanPath: '/tmp' });
    renderPage();
    act(() => useFileStore.setState({ selectedIds: ['f1'] }));
    expect(screen.getByRole('button', { name: '对选中的 1 个文件开始分类' })).toBeEnabled();
  });

  it('opens history view and loads batches', async () => {
    const user = userEvent.setup();
    mocks.getOperationHistory.mockResolvedValue({
      status: 'ok',
      data: {
        batches: [
          {
            batch_id: 'b1',
            op_type: 'move',
            total_count: 2,
            success_count: 2,
            failed_count: 0,
            status: 'done',
            created_at: 'invalid-ts',
            has_delete: false,
            can_undo: true,
          },
        ],
        page: 1,
        page_size: 20,
      },
    });
    renderPage();
    await user.click(screen.getByRole('button', { name: '查看分类历史' }));
    expect(mocks.getOperationHistory).toHaveBeenCalledTimes(1);
    expect(await screen.findByText('分类历史')).toBeInTheDocument();
    expect(screen.getByText('移动 · 2/2 成功')).toBeInTheDocument();
  });

  it('dismisses error alert', async () => {
    const user = userEvent.setup();
    useClassifyStore.setState({ error: '分类失败' });
    renderPage();
    expect(screen.getByRole('alert')).toHaveTextContent('分类失败');
    await user.click(screen.getByLabelText('关闭错误提示'));
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
  });
});
