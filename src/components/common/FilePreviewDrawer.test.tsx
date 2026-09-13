// FilePreviewDrawer 单元测试：文本/图片/PDF/不支持渲染、错误态、关闭回调、
// 以及可选元信息行（file_size/category 缺省时隐藏）。

import { beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import type { FileInfo, FilePreview } from '@/types/ipc';

import { FilePreviewDrawer, type FilePreviewTarget } from './FilePreviewDrawer';

const mocks = vi.hoisted(() => ({
  readFilePreview: vi.fn(),
  readDocumentPreview: vi.fn(),
  Document: vi.fn((props: { children?: unknown }) => props.children),
  Page: vi.fn(() => null),
}));

vi.mock('@/lib/ipc', () => ({
  fileIpc: {
    readFilePreview: mocks.readFilePreview,
    readDocumentPreview: mocks.readDocumentPreview,
  },
}));

vi.mock('react-pdf', () => ({
  pdfjs: { GlobalWorkerOptions: {} },
  Document: mocks.Document,
  Page: mocks.Page,
}));

function makeFile(overrides: Partial<FileInfo> = {}): FileInfo {
  return {
    id: 'a',
    path: '/tmp/a.txt',
    file_name: 'a.txt',
    file_size: 100,
    content_hash: null,
    category: null,
    created_at: '2026-08-01 00:00:00',
    updated_at: '2026-08-01 00:00:00',
    ...overrides,
  };
}

function makePreview(overrides: Partial<FilePreview>): FilePreview {
  return {
    kind: 'Text',
    file_name: 'a.txt',
    file_size: 100,
    text: 'hello',
    data_url: null,
    truncated: false,
    ...overrides,
  };
}

beforeEach(() => {
  mocks.readFilePreview.mockReset();
  mocks.readDocumentPreview.mockReset();
});

describe('FilePreviewDrawer', () => {
  it('renders null when file is null', () => {
    const { container } = render(<FilePreviewDrawer file={null} onClose={vi.fn()} />);
    expect(container).toBeEmptyDOMElement();
  });

  it('renders text preview after load', async () => {
    mocks.readFilePreview.mockResolvedValue({
      status: 'ok',
      data: makePreview({ text: 'hello 世界' }),
    });
    render(<FilePreviewDrawer file={makeFile()} onClose={vi.fn()} />);
    expect(await screen.findByText('hello 世界')).toBeInTheDocument();
    expect(mocks.readFilePreview).toHaveBeenCalledWith('/tmp/a.txt');
  });

  it('renders truncated hint for oversize text', async () => {
    mocks.readFilePreview.mockResolvedValue({
      status: 'ok',
      data: makePreview({ truncated: true, text: 'x' }),
    });
    render(<FilePreviewDrawer file={makeFile()} onClose={vi.fn()} />);
    expect(await screen.findByText(/仅预览前 50MB/)).toBeInTheDocument();
  });

  it('renders error state', async () => {
    mocks.readFilePreview.mockResolvedValue({
      status: 'error',
      error: 'FILE-E-002:预览对象不存在或不是文件',
    });
    render(<FilePreviewDrawer file={makeFile()} onClose={vi.fn()} />);
    expect(await screen.findByText(/FILE-E-002/)).toBeInTheDocument();
  });

  it('renders Office document via readDocumentPreview', async () => {
    mocks.readDocumentPreview.mockResolvedValue({
      status: 'ok',
      data: makePreview({ file_name: 'r.docx', text: '季度报告文本预览' }),
    });
    render(
      <FilePreviewDrawer
        file={makeFile({ path: '/tmp/r.docx', file_name: 'r.docx' })}
        onClose={vi.fn()}
      />,
    );
    expect(await screen.findByText('季度报告文本预览')).toBeInTheDocument();
    expect(mocks.readDocumentPreview).toHaveBeenCalledWith('/tmp/r.docx');
    expect(mocks.readFilePreview).not.toHaveBeenCalled();
  });

  it('renders image preview with data url', async () => {
    mocks.readFilePreview.mockResolvedValue({
      status: 'ok',
      data: makePreview({ kind: 'Image', data_url: 'data:image/png;base64,AAA' }),
    });
    render(<FilePreviewDrawer file={makeFile()} onClose={vi.fn()} />);
    const img = await screen.findByRole('img');
    expect(img).toHaveAttribute('src', 'data:image/png;base64,AAA');
  });

  it('renders pdf document with data url', async () => {
    mocks.readFilePreview.mockResolvedValue({
      status: 'ok',
      data: makePreview({ kind: 'Pdf', data_url: 'data:application/pdf;base64,BBB' }),
    });
    render(<FilePreviewDrawer file={makeFile()} onClose={vi.fn()} />);
    await waitFor(() => {
      const props = mocks.Document.mock.calls[0]?.[0] as { file?: string } | undefined;
      expect(props).toEqual(expect.objectContaining({ file: 'data:application/pdf;base64,BBB' }));
    });
  });

  it('syncs pageNumber when initialPage changes for same file', async () => {
    // FE-M6：ChatPage 对 Drawer 用 key={file.id}，同文件不同页码的引用点击不会重挂载，
    // initialPage 变化需要在 render 期同步到 pageNumber（否则停留旧页）
    mocks.readFilePreview.mockResolvedValue({
      status: 'ok',
      data: makePreview({ kind: 'Pdf', data_url: 'data:application/pdf;base64,BBB' }),
    });
    const file = makeFile({ path: '/tmp/a.pdf', file_name: 'a.pdf' });
    const { rerender } = render(
      <FilePreviewDrawer file={file} onClose={vi.fn()} initialPage={2} />,
    );
    expect(await screen.findByText('2 / …')).toBeInTheDocument();

    rerender(<FilePreviewDrawer file={file} onClose={vi.fn()} initialPage={3} />);
    expect(await screen.findByText('3 / …')).toBeInTheDocument();
    // 同引用不重挂载：不应重新拉取预览（走 render 期同步而非重新加载）
    expect(mocks.readFilePreview).toHaveBeenCalledTimes(1);
  });

  it('renders unsupported fallback', async () => {
    mocks.readFilePreview.mockResolvedValue({
      status: 'ok',
      data: makePreview({ kind: 'Unsupported' }),
    });
    render(<FilePreviewDrawer file={makeFile()} onClose={vi.fn()} />);
    expect(await screen.findByText(/暂不支持预览/)).toBeInTheDocument();
  });

  it('shows size and category meta rows when provided', async () => {
    mocks.readFilePreview.mockResolvedValue({ status: 'ok', data: makePreview({ text: 'x' }) });
    render(<FilePreviewDrawer file={makeFile({ category: '财务' })} onClose={vi.fn()} />);
    await screen.findByText('x');
    expect(screen.getByText('100 B')).toBeInTheDocument();
    expect(screen.getByText('财务')).toBeInTheDocument();
    expect(screen.getByText('/tmp/a.txt')).toBeInTheDocument();
  });

  it('hides size and category meta rows when omitted', async () => {
    mocks.readFilePreview.mockResolvedValue({ status: 'ok', data: makePreview({ text: 'x' }) });
    // 智能分类场景：只有 path + file_name，无 size/category 元信息
    const target: FilePreviewTarget = { path: '/tmp/a.txt', file_name: 'a.txt' };
    render(<FilePreviewDrawer file={target} onClose={vi.fn()} />);
    await screen.findByText('x');
    expect(screen.getByText('/tmp/a.txt')).toBeInTheDocument();
    expect(screen.queryByText('100 B')).not.toBeInTheDocument();
    expect(screen.queryByText(/未分类/)).not.toBeInTheDocument();
  });

  it('calls onClose when close button clicked', async () => {
    mocks.readFilePreview.mockResolvedValue({ status: 'ok', data: makePreview({ text: 'x' }) });
    const onClose = vi.fn();
    render(<FilePreviewDrawer file={makeFile()} onClose={onClose} />);
    await screen.findByText('x');
    await userEvent.click(screen.getByLabelText('关闭预览'));
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});
