// ChatPage 单元测试：空态、文件计数、建立索引成功/失败、消息渲染与清空、错误提示。

import { beforeEach, describe, expect, it, vi } from 'vitest';
import { StrictMode } from 'react';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { ChatRole, type ChatMessage } from '@/types/models';
import type { FileInfo } from '@/types/ipc';

import { useChatStore } from '@/stores/chatStore';
import { useFileStore } from '@/stores/fileStore';
import { useSidecarStore } from '@/stores/sidecarStore';
import { ChatPage } from './ChatPage';

const mocks = vi.hoisted(() => ({
  readFilePreview: vi.fn(),
  searchByFilename: vi.fn(),
  listAllFiles: vi.fn(),
  buildIndex: vi.fn(),
  chatStream: vi.fn(),
  listenChatEvent: vi.fn(),
  retrySidecarStart: vi.fn(),
  Document: vi.fn((props: { children?: unknown }) => props.children),
  Page: vi.fn(() => null),
}));

vi.mock('@/lib/ipc', () => ({
  fileIpc: {
    readFilePreview: mocks.readFilePreview,
    searchByFilename: mocks.searchByFilename,
    listAllFiles: mocks.listAllFiles,
    buildIndex: mocks.buildIndex,
  },
}));

vi.mock('@/lib/ipc/chatIpc', () => ({
  chatStream: mocks.chatStream,
  listenChatEvent: mocks.listenChatEvent,
}));

// P1-1：sidecarStore 通过命令重试；测试环境无 Tauri，mock 掉 retrySidecarStart。
// 本文件仅以 type-only 方式引用 '@/types/ipc'（运行期已擦除），全量 mock 不影响。
vi.mock('@/types/ipc', () => ({
  commands: {
    retrySidecarStart: mocks.retrySidecarStart,
  },
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
  // 默认返回空文件列表——ChatPage 初始化 loadAllFiles() 需要该 mock。
  // 这样 ChatPage useEffect 异步 promise resolve 后不会抛 unhandled rejection
  // （Vitest 把"测试结束后仍 pending/resolve 但状态被 mock 没定义"当 Unhandled Errors）。
  mocks.listAllFiles.mockResolvedValue({ status: 'ok', data: [], total: 0 });
  // 预览 IPC 默认返回 Unsupported——防止 readFilePreview() 返回 undefined
  // 导致 FilePreviewDrawer useEffect 里 .then 访问 undefined.then 抛 uncaught。
  mocks.readFilePreview.mockResolvedValue({
    status: 'ok',
    data: {
      kind: 'Unsupported',
      file_name: '',
      file_size: 0,
      text: null,
      data_url: null,
      truncated: false,
    },
  });
  useFileStore.setState({ files: [], total: 0 });
  // P1-1：默认引擎就绪，让既有「建索引」用例按原行为执行；引擎未就绪的
  // 禁用态由独立用例显式 setState 覆盖验证。
  useSidecarStore.setState({ status: 'ready', message: null });
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

  it('disables AI features while engine is not ready', () => {
    // P1-1：引擎启动中 → 建索引按钮禁用 + 输入框禁用 + 提示可见
    useSidecarStore.setState({ status: 'starting', message: null });
    renderPage();
    expect(screen.getByTestId('build-index')).toBeDisabled();
    expect(screen.getByPlaceholderText('输入问题，Enter 发送，Shift+Enter 换行')).toBeDisabled();
    expect(screen.getByText('AI 引擎启动中，就绪后可开始问答…')).toBeInTheDocument();
  });

  it('shows retry entry when engine failed', async () => {
    // P1-1：引擎失败 → 提示「重试」入口，点击后乐观转 starting
    mocks.retrySidecarStart.mockResolvedValue({ status: 'ok', data: null });
    useSidecarStore.setState({ status: 'failed', message: '握手失败' });
    renderPage();
    const retryButton = screen.getByRole('button', { name: '重试' });
    expect(retryButton).toBeInTheDocument();
    await userEvent.click(retryButton);
    expect(useSidecarStore.getState().status).toBe('starting');
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

  it('opens preview overlay dialog when citation chip clicked', async () => {
    const user = userEvent.setup();
    const file: FileInfo = {
      id: 'f1',
      path: '/Users/demo/Documents/notes/设计规范.md',
      file_name: '设计规范.md',
      file_size: 1024,
      content_hash: null,
      category: '文档',
      created_at: '',
      updated_at: '',
    };
    const answer: ChatMessage = {
      id: 'm2',
      role: ChatRole.Assistant,
      content: '答案',
      citations: [{ id: 1, fileName: '设计规范.md', page: 2, text: '引用片段' }],
      createdAt: '',
    };
    seed({ messages: [msg, answer] });
    useFileStore.setState({ files: [file], total: 1 });
    mocks.readFilePreview.mockResolvedValue({
      status: 'ok',
      data: {
        kind: 'Text',
        file_name: '设计规范.md',
        file_size: 1024,
        text: '# 设计规范正文',
        data_url: null,
        truncated: false,
      },
    });

    renderPage();
    await user.click(screen.getByRole('button', { name: /设计规范\.md/ }));

    // 右侧覆盖层弹窗（role=dialog）应出现：标题 + 正文预览 + 路径
    expect(await screen.findByRole('dialog')).toBeInTheDocument();
    expect(screen.getByLabelText('文件预览')).toBeInTheDocument();
    expect(await screen.findByText('# 设计规范正文')).toBeInTheDocument();
    // 抽屉头部 title 与底部 meta 各有一处完整路径
    expect(
      screen.getAllByTitle('/Users/demo/Documents/notes/设计规范.md').length,
    ).toBeGreaterThanOrEqual(1);
  });

  // FE-m14 回归：StrictMode（dev 双跑 setup→cleanup→setup）下 isMountedRef
  // 曾因 cleanup-only 写法永久停留在 false，点击引用静默 return、抽屉不弹。
  // 用 StrictMode 包裹渲染复现真实 dev 行为。
  it('opens preview when citation clicked under StrictMode', async () => {
    const user = userEvent.setup();
    const file: FileInfo = {
      id: 'f1',
      path: '/Users/demo/Documents/notes/设计规范.md',
      file_name: '设计规范.md',
      file_size: 1024,
      content_hash: null,
      category: '文档',
      created_at: '',
      updated_at: '',
    };
    const answer: ChatMessage = {
      id: 'm2',
      role: ChatRole.Assistant,
      content: '答案',
      citations: [{ id: 1, fileName: '设计规范.md', page: 2, text: '引用片段' }],
      createdAt: '',
    };
    seed({ messages: [msg, answer] });
    useFileStore.setState({ files: [file], total: 1 });

    render(
      <StrictMode>
        <ChatPage />
      </StrictMode>,
    );
    await user.click(screen.getByRole('button', { name: /设计规范\.md/ }));

    expect(await screen.findByRole('dialog')).toBeInTheDocument();
  });
});
