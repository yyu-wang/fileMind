// ChatPage 单元测试：空态、文件计数、建立索引成功/失败、消息渲染与清空、错误提示。

import { beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { ChatRole, type ChatMessage } from '@/types/models';

import { useChatStore } from '@/stores/chatStore';
import { useFileStore } from '@/stores/fileStore';
import { ChatPage } from './ChatPage';

const mocks = vi.hoisted(() => ({
  readFilePreview: vi.fn(),
  searchByFilename: vi.fn(),
  buildIndex: vi.fn(),
  chatStream: vi.fn(),
  listenChatEvent: vi.fn(),
  Document: vi.fn((props: { children?: unknown }) => props.children),
  Page: vi.fn(() => null),
}));

vi.mock('@/lib/ipc', () => ({
  fileIpc: {
    readFilePreview: mocks.readFilePreview,
    searchByFilename: mocks.searchByFilename,
    buildIndex: mocks.buildIndex,
  },
}));

vi.mock('@/lib/ipc/chatIpc', () => ({
  chatStream: mocks.chatStream,
  listenChatEvent: mocks.listenChatEvent,
}));

vi.mock('react-pdf', () => ({
  pdfjs: { GlobalWorkerOptions: {} },
  Document: mocks.Document,
  Page: mocks.Page,
}));

const msg: ChatMessage = {
  id: 'm1',
  role: ChatRole.User,
  content: '你好',
  createdAt: '',
};

function seed(overrides: Partial<Parameters<typeof useChatStore.setState>[0]> = {}): void {
  localStorage.clear();
  vi.clearAllMocks();
  useFileStore.setState({ files: [], total: 0 });
  useChatStore.setState({
    messages: [],
    isStreaming: false,
    currentStream: '',
    error: null,
    status: 'idle',
    rewrittenQuery: null,
    searchInfo: null,
    retries: 0,
    retryReason: null,
    lowConfidence: false,
    pendingCitations: [],
    ...overrides,
  });
}

function renderPage(): void {
  render(<ChatPage />);
}

beforeEach(() => seed());

describe('ChatPage', () => {
  it('shows empty state and file count', () => {
    seed({ messages: [] });
    useFileStore.setState({ total: 42 });
    renderPage();
    expect(screen.getByText('开始知识问答')).toBeInTheDocument();
    expect(screen.getByText('基于 42 个已索引文件')).toBeInTheDocument();
  });

  it('builds index successfully and shows summary', async () => {
    const user = userEvent.setup();
    mocks.buildIndex.mockResolvedValue({
      status: 'ok',
      data: { indexed_count: 10, skipped_count: 2 },
    });
    renderPage();
    await user.click(screen.getByTestId('build-index'));
    expect(mocks.buildIndex).toHaveBeenCalledTimes(1);
    expect(await screen.findByText('索引完成：10 个文件，跳过 2 个')).toBeInTheDocument();
  });

  it('shows error message when build index fails', async () => {
    const user = userEvent.setup();
    mocks.buildIndex.mockResolvedValue({ status: 'error', error: '向量化失败' });
    renderPage();
    await user.click(screen.getByTestId('build-index'));
    expect(await screen.findByText('索引失败：向量化失败')).toBeInTheDocument();
  });

  it('renders messages and 清空对话 clears them', async () => {
    const user = userEvent.setup();
    seed({ messages: [msg] });
    renderPage();
    expect(screen.getByText('你好')).toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: '🧹 清空对话' }));
    expect(useChatStore.getState().messages).toEqual([]);
    expect(screen.getByText('开始知识问答')).toBeInTheDocument();
  });

  it('hides 清空对话 when no messages', () => {
    renderPage();
    expect(screen.queryByRole('button', { name: '🧹 清空对话' })).not.toBeInTheDocument();
  });

  it('dismisses error alert', async () => {
    const user = userEvent.setup();
    seed({ error: '请求失败' });
    renderPage();
    expect(screen.getByRole('alert')).toHaveTextContent('请求失败');
    await user.click(screen.getByLabelText('关闭错误提示'));
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
  });
});
