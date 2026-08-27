// 知识问答页（设计稿 §6 / T6.6）：对话气泡 + 流式光标 + 引用标签 + 检索状态栏。
//
// 引用跳转：按文件名从 fileStore 匹配 FileInfo，兜底 fileIpc.searchByFilename，
// 打开 FilePreviewDrawer 定位到引用页码。

import { useEffect, useRef, useState } from 'react';
import { ChatBubble } from '@/components/chat/ChatBubble';
import { ChatInput } from '@/components/chat/ChatInput';
import { SearchStatusBar } from '@/components/chat/SearchStatusBar';
import { FilePreviewDrawer } from '@/components/common/FilePreviewDrawer';
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

/** 按文件名查找 FileInfo：本地列表优先，兜底走 Rust 文件名搜索（多策略匹配）。 */
async function findFileByName(
  fileName: string,
  fallbackFiles: FileInfo[],
): Promise<FileInfo | null> {
  const nameLower = fileName.toLowerCase();
  const nameNoExt = nameLower.replace(/\.[^.]+$/, '');

  // 1) 精确匹配（大小写敏感→不敏感）
  const exactHit = fallbackFiles.find((f) => f.file_name === fileName);
  if (exactHit) return exactHit;
  const ciHit = fallbackFiles.find((f) => f.file_name.toLowerCase() === nameLower);
  if (ciHit) return ciHit;

  // 2) 去扩展名匹配（数据库里是 .md/.txt 但 citation 丢了后缀）
  if (nameNoExt.length > 0) {
    const noExtHit = fallbackFiles.find((f) => {
      const base = f.file_name.toLowerCase().replace(/\.[^.]+$/, '');
      return base === nameNoExt || base === nameLower;
    });
    if (noExtHit) return noExtHit;
  }

  // 3) 共享前缀/子串包含匹配——应对 UI 里文件名为了显示会追加 "…" 但
  //    citation.fileName 实际是完整的；主要场景是文件名中带 emoji/-/_ 变体。
  const looseHit = fallbackFiles.find((f) => {
    const db = f.file_name.toLowerCase();
    return db.includes(nameLower) || nameLower.includes(db);
  });
  if (looseHit) return looseHit;

  // 4) 兜底：Rust FTS/文件名 LIKE 模糊搜索
  const result = await fileIpc.searchByFilename(fileName, 3);
  if (result.status === 'ok' && result.data.length > 0) {
    // 返回结果中优先挑和原文件名相似度最高的（命中包含后缀全匹配）
    return result.data.find((f) => f.file_name.toLowerCase() === nameLower) ?? result.data[0];
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
  const filesReady = useFileStore((s) => !s.isScanning);
  const loadAllFiles = useFileStore((s) => s.loadAllFiles);

  const [previewTarget, setPreviewTarget] = useState<PreviewTarget | null>(null);
  // 预览抽屉挂载 key：每次打开/切引用 +1，保证 react-pdf 的 Document/Page 组件彻底重挂载，
  // 避免 PDF 缩放/翻页后残留状态导致下一次点击 phase='ready' 不刷新 & 页面卡死。
  const [previewKey, setPreviewKey] = useState(0);
  // 建立索引：请求状态 + 结果提示（T7.x）
  const [building, setBuilding] = useState(false);
  const [indexMessage, setIndexMessage] = useState<string | null>(null);
  // FE-m14：await 后 setState 的卸载守卫，防组件卸载后 setState warning
  const isMountedRef = useRef(true);
  useEffect(() => {
    return () => {
      isMountedRef.current = false;
    };
  }, []);

  // ChatPage 可能是用户进入 App 的第一个路由页（直接导航 / 链接跳 / 刷新），
  // fileStore.files 初始化是 []，本地精确匹配永远失败；在此兜底拉一次全量列表。
  useEffect(() => {
    if (files.length === 0 && filesReady) {
      void loadAllFiles();
    }
  }, [files.length, filesReady, loadAllFiles]);

  // T6.10 快捷键：⌘N 新建对话（清空当前会话）
  useHotkeys([{ key: 'n', meta: true, handler: clearHistory }]);

  const handleBuildIndex = async () => {
    setBuilding(true);
    setIndexMessage(null);
    try {
      const result = await fileIpc.buildIndex();
      if (!isMountedRef.current) return;
      if (result.status === 'ok') {
        setIndexMessage(
          `索引完成：${result.data.indexed_count} 个文件，跳过 ${result.data.skipped_count} 个`,
        );
      } else {
        setIndexMessage(`索引失败：${result.error}`);
      }
    } catch (err) {
      if (!isMountedRef.current) return;
      setIndexMessage(`构建索引时出错：${err instanceof Error ? err.message : String(err)}`);
    } finally {
      setBuilding(false);
    }
  };

  const handleCitationClick = async (citation: ChatCitation) => {
    const file = await findFileByName(citation.fileName, files);
    if (!isMountedRef.current) return;
    if (file) {
      // FE-M6（修复 PDF 缩放后再次点击无响应）：先写 null 卸载上一次实例，
      // 再 +1 换 key 再写 target → 保证 phase 从 loading 重新开始，
      // 杜绝 react-pdf Document 复用旧 canvas/worker 导致的空白/卡死。
      setPreviewTarget(null);
      setPreviewKey((k) => k + 1);
      // 下一帧再挂载：让 null 状态 flush 一次，React 才会真正重建组件树
      window.requestAnimationFrame(() => {
        if (!isMountedRef.current) return;
        setPreviewTarget({ file, initialPage: citation.page });
      });
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
    <div className="page chat-page">
      <header className="chat-header">
        <span className="title">💬 知识问答</span>
        <span className="meta">基于 {totalFiles} 个已索引文件</span>
        <div className="header-actions">
          {/* T7.x：建立索引（向量化入库 LanceDB，问答检索的数据源） */}
          <button
            type="button"
            className="btn btn--ghost btn--sm"
            data-testid="build-index"
            onClick={() => void handleBuildIndex()}
            disabled={building}
          >
            {building ? '索引中…' : '建立索引'}
          </button>
          {messages.length > 0 && (
            <button type="button" className="btn btn--ghost btn--sm" onClick={clearHistory}>
              🧹 清空对话
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

      {/* 主体：消息区 + 预览抽屉并排（flex-row），防止预览被挤出视口外 */}
      <div className="chat-page__main">
        <div className="chat-page__content">
          {messages.length === 0 && !isStreaming ? (
            <div className="chat-empty">
              <div className="icon">💬</div>
              <h3>开始知识问答</h3>
              <p>基于已索引文档回答问题，答案会标注可跳转的引用来源；多轮对话自动带入上下文</p>
            </div>
          ) : (
            <div className="chat-messages">
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

          <SearchStatusBar
            status={status}
            rewrittenQuery={rewrittenQuery}
            searchInfo={searchInfo}
            retries={retries}
            retryReason={retryReason}
            lowConfidence={lowConfidence}
            hasTokens={currentStream.length > 0}
          />
        </div>

        <FilePreviewDrawer
          key={previewKey}
          file={previewTarget?.file ?? null}
          {...(previewTarget ? { initialPage: previewTarget.initialPage } : {})}
          onClose={() => setPreviewTarget(null)}
        />
      </div>

      <div className="chat-input-area">
        <ChatInput disabled={isStreaming} onSend={(content) => void sendMessage(content)} />
      </div>
    </div>
  );
}
