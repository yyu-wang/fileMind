// FilesPage 单元测试：空态/列表渲染、扫描中禁用、筛选下拉、扫描/刷新动作与错误提示。

import { beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter } from 'react-router-dom';
import type { FileInfo } from '@/types/ipc';

import { useFileStore } from '@/stores/fileStore';
import { FilesPage } from './FilesPage';

const mocks = vi.hoisted(() => ({
  readFilePreview: vi.fn(),
  scanDirectory: vi.fn(),
  listAllFiles: vi.fn(),
  getFileStats: vi.fn(),
  e2eGetTestDir: vi.fn(),
  open: vi.fn(),
  Document: vi.fn((props: { children?: unknown }) => props.children),
  Page: vi.fn(() => null),
}));

vi.mock('@/lib/ipc', () => ({
  fileIpc: {
    readFilePreview: mocks.readFilePreview,
    scanDirectory: mocks.scanDirectory,
    listAllFiles: mocks.listAllFiles,
    getFileStats: mocks.getFileStats,
    e2eGetTestDir: mocks.e2eGetTestDir,
  },
}));

vi.mock('react-pdf', () => ({
  pdfjs: { GlobalWorkerOptions: {} },
  Document: mocks.Document,
  Page: mocks.Page,
}));

vi.mock('@tauri-apps/plugin-dialog', () => ({ open: mocks.open }));

function file(overrides: Partial<FileInfo> = {}): FileInfo {
  return {
    id: 'f1',
    path: '/tmp/f1.pdf',
    file_name: 'f1.pdf',
    file_size: 100,
    content_hash: null,
    category: null,
    created_at: '2026-08-01 00:00:00',
    updated_at: '2026-08-01 00:00:00',
    ...overrides,
  };
}

function seed(overrides: Partial<Parameters<typeof useFileStore.setState>[0]> = {}): void {
  localStorage.clear();
  vi.clearAllMocks();
  useFileStore.setState({
    files: [],
    scanPath: null,
    isScanning: false,
    selectedIds: [],
    error: null,
    total: 0,
    ...overrides,
  });
  mocks.e2eGetTestDir.mockResolvedValue(null);
  mocks.getFileStats.mockResolvedValue({ status: 'ok', data: { total_files: 0 } });
}

function renderPage(): void {
  render(
    <MemoryRouter>
      <FilesPage />
    </MemoryRouter>,
  );
}

beforeEach(() => seed());

describe('FilesPage', () => {
  it('shows empty state before any scan', () => {
    renderPage();
    expect(screen.getByText('尚未扫描目录')).toBeInTheDocument();
    expect(screen.getByTestId('files-scan')).toBeInTheDocument();
  });

  it('shows scan path and file rows when files exist', () => {
    seed({
      files: [file(), file({ id: 'f2', file_name: 'b.txt', path: '/tmp/b.txt', category: null })],
      scanPath: '/tmp',
    });
    renderPage();
    expect(screen.getByText('/tmp')).toBeInTheDocument();
    expect(screen.getByText('f1.pdf')).toBeInTheDocument();
    expect(screen.getByText('b.txt')).toBeInTheDocument();
  });

  it('disables scan button while scanning', () => {
    seed({ isScanning: true, scanPath: '/tmp' });
    renderPage();
    expect(screen.getByTestId('files-scan')).toBeDisabled();
    expect(screen.getByText('扫描中…')).toBeInTheDocument();
  });

  it('builds category filter options from files', () => {
    seed({
      files: [file({ category: '文档' }), file({ id: 'f2', category: '图片' })],
      scanPath: '/tmp',
    });
    renderPage();
    const select = screen.getByLabelText('按分类筛选');
    expect(select).toHaveTextContent('全部分类');
    expect(select).toHaveTextContent('文档');
    expect(select).toHaveTextContent('图片');
  });

  it('disables 整理选中 when nothing selected and enables it with selection', () => {
    seed({ files: [file()], scanPath: '/tmp', selectedIds: [] });
    renderPage();
    expect(screen.getByRole('button', { name: /整理选中/ })).toBeDisabled();
  });

  it('enables 整理选中 when files are selected', () => {
    seed({ files: [file()], scanPath: '/tmp', selectedIds: ['f1'] });
    renderPage();
    const btn = screen.getByRole('button', { name: /整理选中/ });
    expect(btn).not.toBeDisabled();
    expect(btn).toHaveTextContent('整理选中 (1)');
  });

  it('scans via E2E test dir when present instead of native dialog', async () => {
    const user = userEvent.setup();
    mocks.e2eGetTestDir.mockResolvedValue('/e2e/data');
    mocks.scanDirectory.mockResolvedValue({ status: 'ok', data: [file()] });
    renderPage();
    await user.click(screen.getByTestId('files-scan'));
    expect(mocks.scanDirectory).toHaveBeenCalledWith('/e2e/data');
    expect(mocks.open).not.toHaveBeenCalled();
  });

  it('refresh triggers loadAllFiles', async () => {
    const user = userEvent.setup();
    mocks.listAllFiles.mockResolvedValue({ status: 'ok', data: [] });
    renderPage();
    await user.click(screen.getByRole('button', { name: '刷新' }));
    expect(mocks.listAllFiles).toHaveBeenCalledWith(null);
  });

  it('dismisses error alert', async () => {
    const user = userEvent.setup();
    seed({ error: '扫描失败' });
    renderPage();
    expect(screen.getByRole('alert')).toHaveTextContent('扫描失败');
    await user.click(screen.getByLabelText('关闭错误提示'));
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
  });

  it('hides 清除选中 button when nothing is selected', () => {
    seed({ files: [file()], scanPath: '/tmp', selectedIds: [] });
    renderPage();
    expect(screen.queryByTestId('files-clear-selection')).not.toBeInTheDocument();
  });

  it('shows 清除选中 with count and clears selection on click', async () => {
    const user = userEvent.setup();
    seed({ files: [file()], scanPath: '/tmp', selectedIds: ['f1'] });
    renderPage();
    const btn = screen.getByTestId('files-clear-selection');
    expect(btn).toHaveTextContent('清除选中');
    await user.click(btn);
    expect(useFileStore.getState().selectedIds).toEqual([]);
    expect(screen.queryByTestId('files-clear-selection')).not.toBeInTheDocument();
  });
});
