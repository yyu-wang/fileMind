import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import type { FileInfo, FilePreview } from '@/types/ipc';

const mocks = vi.hoisted(() => ({
  readFilePreview: vi.fn(),
  Document: vi.fn((props: { children?: unknown }) => props.children),
  Page: vi.fn(() => null),
}));

vi.mock('@/lib/ipc', () => ({
  fileIpc: { readFilePreview: mocks.readFilePreview },
}));

vi.mock('react-pdf', () => ({
  pdfjs: { GlobalWorkerOptions: {} },
  Document: mocks.Document,
  Page: mocks.Page,
}));

import { FilePreviewDrawer } from './FilePreviewDrawer';

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
    expect(await screen.findByText(/仅预览前 512KB/)).toBeInTheDocument();
  });

  it('renders error state', async () => {
    mocks.readFilePreview.mockResolvedValue({
      status: 'error',
      error: 'FILE-E-002:预览对象不存在或不是文件',
    });
    render(<FilePreviewDrawer file={makeFile()} onClose={vi.fn()} />);
    expect(await screen.findByText(/FILE-E-002/)).toBeInTheDocument();
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

  it('renders unsupported fallback', async () => {
    mocks.readFilePreview.mockResolvedValue({
      status: 'ok',
      data: makePreview({ kind: 'Unsupported' }),
    });
    render(<FilePreviewDrawer file={makeFile()} onClose={vi.fn()} />);
    expect(await screen.findByText(/暂不支持预览/)).toBeInTheDocument();
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
