// 知识问答页（设计稿 §6 / T6.6）：对话气泡 + 流式光标 + 引用标签 + 检索状态栏。
//
// 引用跳转：按文件名从 fileStore 匹配 FileInfo，兜底 fileIpc.searchByFilename，
// 打开 FilePreviewDrawer 定位到引用页码。

import { useState } from 'react';
import { ChatBubble } from '@/components/chat/ChatBubble';
import { ChatInput } from '@/components/chat/ChatInput';
import { SearchStatusBar } from '@/components/chat/SearchStatusBar';
import { FilePreviewDrawer } from '@/components/file/FilePreviewDrawer';
import { useHotkeys } from '@/hooks/useHotkeys';
import { fileIpc } from '@/lib/ipc';
import { useChatStore } from '@/stores/chatStore';
import { useFileStore } from '@/stores/fileStore';
import { ChatRole, type ChatCitation, type ChatMessage } from '@/types/models';
import type { FileInfo } from '@/types/ipc';

interface PreviewTarget {
  file: FileInfo;
  initialPage: number;
}

/** 按文件名查找 FileInfo：本地列表优先，兜底走 Rust 文件名搜索。 */
async function findFileByName(fileName: string): Promise<FileInfo | null> {
  const result = await fileIpc.searchByFilename(fileName, 1);
  if (result.status === 'ok' && result.data.length > 0) {
    return result.data[0];
  }
  return null;
}

export function ChatPage() {
  const messages = useChatStore((s) => s.messages);
  const isStreaming = useChatStore((s) => s.isStreaming);
  const currentStream = useChatStore((s) => s.currentStream);
  const status = useChatStore((s) => s.status);
  const rewrittenQuery = useChatStore((s) => s.rewrittenQuery);
  const searchInfo = useChatStore((s) => s.searchInfo);
  const retries = useChatStore((s) => s.retries);
  const retryReason = useChatStore((s) => s.retryReason);
  const lowConfidence = useChatStore((s) => s.lowConfidence);
  const pendingCitations = useChatStore((s) => s.pendingCitations);
  const error = useChatStore((s) => s.error);
  const sendMessage = useChatStore((s) => s.sendMessage);
  const clearHistory = useChatStore((s) => s.clearHistory);
  const clearError = useChatStore((s) => s.clearError);
  const totalFiles = useFileStore((s) => s.total);
  const files = useFileStore((s) => s.files);

  const [previewTarget, setPreviewTarget] = useState<PreviewTarget | null>(null);
  // 建立索引：请求状态 + 结果提示（T7.x）
  const [building, setBuilding] = useState(false);
  const [indexMessage, setIndexMessage] = useState<string | null>(null);

  // T6.10 快捷键：⌘N 新建对话（清空当前会话）
  useHotkeys([{ key: 'n', meta: true, handler: clearHistory }]);

  const handleBuildIndex = async () => {
    setBuilding(true);
    setIndexMessage(null);
    const result = await fileIpc.buildIndex();
    if (result.status === 'ok') {
      setIndexMessage(
        `索引完成：${result.data.indexed_count} 个文件，跳过 ${result.data.skipped_count} 个`,
      );
    } else {
      setIndexMessage(result.error);
    }
    setBuilding(false);
  };

  const handleCitationClick = async (citation: ChatCitation) => {
    const local = files.find((f) => f.file_name === citation.fileName);
    const file = local ?? (await findFileByName(citation.fileName));
    if (file) {
      setPreviewTarget({ file, initialPage: citation.page });
    }
  };

  const streamingMessage: ChatMessage | null = isStreaming
    ? {
        id: 'streaming',
        role: ChatRole.Assistant,
        content: currentStream,
        ...(pendingCitations.length > 0 ? { citations: pendingCitations } : {}),
        ...(lowConfidence ? { lowConfidence: true } : {}),
        ...(retries > 0 ? { retries } : {}),
        createdAt: '',
      }
    : null;

  return (
    <div className="chat-page">
      <header className="chat-page__header">
        <h1 className="chat-page__title">知识问答</h1>
        <span className="chat-page__count">共 {totalFiles} 个文件</span>
        <div className="chat-page__header-actions">
          {/* T7.x：建立索引（向量化入库 LanceDB，问答检索的数据源） */}
          <button
            type="button"
            className="btn btn--ghost"
            onClick={() => void handleBuildIndex()}
            disabled={building}
          >
            {building ? '索引中…' : '建立索引'}
          </button>
          {messages.length > 0 && (
            <button
              type="button"
              className="btn btn--ghost chat-page__clear"
              onClick={clearHistory}
            >
              清空对话
            </button>
          )}
        </div>
      </header>

      {indexMessage && (
        <div className="chat-page__index-tip" role="status">
          <span>{indexMessage}</span>
        </div>
      )}

      {error && (
        <div className="chat-page__error" role="alert">
          <span>{error}</span>
          <button
            type="button"
            className="chat-page__error-dismiss"
            aria-label="关闭错误提示"
            onClick={clearError}
          >
            ×
          </button>
        </div>
      )}

      <div className="chat-page__body">
        {messages.length === 0 && !isStreaming ? (
          <div className="chat-page__empty">
            <p className="chat-page__empty-title">开始知识问答</p>
            <p className="chat-page__empty-sub">
              基于已索引文档回答问题，答案会标注可跳转的引用来源；多轮对话自动带入上下文
            </p>
          </div>
        ) : (
          <div className="chat-page__messages">
            {messages.map((message) => (
              <ChatBubble
                key={message.id}
                message={message}
                onCitationClick={(c) => void handleCitationClick(c)}
              />
            ))}
            {streamingMessage && (
              <ChatBubble
                key="streaming"
                message={streamingMessage}
                streaming
                onCitationClick={(c) => void handleCitationClick(c)}
              />
            )}
          </div>
        )}
      </div>

      <SearchStatusBar
        status={status}
        rewrittenQuery={rewrittenQuery}
        searchInfo={searchInfo}
        retries={retries}
        retryReason={retryReason}
        lowConfidence={lowConfidence}
        hasTokens={currentStream.length > 0}
      />

      <div className="chat-page__input">
        <ChatInput disabled={isStreaming} onSend={(content) => void sendMessage(content)} />
      </div>

      <FilePreviewDrawer
        key={previewTarget?.file.id ?? 'none'}
        file={previewTarget?.file ?? null}
        {...(previewTarget ? { initialPage: previewTarget.initialPage } : {})}
        onClose={() => setPreviewTarget(null)}
      />
    </div>
  );
}
