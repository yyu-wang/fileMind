// 知识问答页（设计稿 §6 / T6.6）：对话气泡 + 流式光标 + 引用标签 + 检索状态栏。
//
// 引用跳转：按文件名从 fileStore 匹配 FileInfo（findFileByName，见 lib/citation），
// 兜底 fileIpc.searchByFilename，打开 FilePreviewDrawer 定位到引用页码。
//
// 渲染性能：本页只订阅低频 state（messages/isStreaming/error 等）；
// token 级高频 state（currentStream/status/searchInfo 等）由 ChatMessageList
// 内部 StreamingBubble 与 SearchStatusBar 自行订阅，流式输出不整页重渲。

import { useCallback, useEffect, useRef, useState } from 'react';
import { ChatInput } from '@/components/chat/ChatInput';
import { ChatMessageList } from '@/components/chat/ChatMessageList';
import { SearchStatusBar } from '@/components/chat/SearchStatusBar';
import { LazyFilePreviewDrawer } from '@/components/common/LazyFilePreviewDrawer';
import { useToastStore } from '@/components/ui/Toast';
import { useHotkeys } from '@/hooks/useHotkeys';
import { findFileByName } from '@/lib/citation';
import { fileIpc } from '@/lib/ipc';
import { useChatStore } from '@/stores/chatStore';
import { useFileStore } from '@/stores/fileStore';
import { useSidecarStore } from '@/stores/sidecarStore';
import { type ChatCitation } from '@/types/models';
import type { FileInfo } from '@/types/ipc';

interface PreviewTarget {
  file: FileInfo;
  initialPage: number;
}

export function ChatPage() {
  const messages = useChatStore((s) => s.messages);
  const isStreaming = useChatStore((s) => s.isStreaming);
  const error = useChatStore((s) => s.error);
  const sendMessage = useChatStore((s) => s.sendMessage);
  const clearHistory = useChatStore((s) => s.clearHistory);
  const clearError = useChatStore((s) => s.clearError);
  const totalFiles = useFileStore((s) => s.total);
  const filesReady = useFileStore((s) => !s.isScanning);
  const loadAllFiles = useFileStore((s) => s.loadAllFiles);
  // P1-1：AI 引擎未就绪时禁用问答/建索引（避免「点了没反应」）
  const engineReady = useSidecarStore((s) => s.status === 'ready');
  const engineFailed = useSidecarStore((s) => s.status === 'failed' || s.status === 'crash_loop');
  const retrySidecar = useSidecarStore((s) => s.retryStart);

  const [previewTarget, setPreviewTarget] = useState<PreviewTarget | null>(null);
  // 预览抽屉挂载 key：每次打开/切引用 +1，保证 react-pdf 的 Document/Page 组件彻底重挂载，
  // 避免 PDF 缩放/翻页后残留状态导致下一次点击 phase='ready' 不刷新 & 页面卡死。
  const [previewKey, setPreviewKey] = useState(0);
  // 建立索引：请求状态 + 结果提示（T7.x）
  const [building, setBuilding] = useState(false);
  const [indexMessage, setIndexMessage] = useState<string | null>(null);
  // FE-m14：await 后 setState 的卸载守卫，防组件卸载后 setState warning。
  // 注意必须在 setup 里显式置 true：StrictMode（dev 双跑 setup→cleanup→setup）
  // 和 Vite HMR Fast Refresh（重跑 effect 但保留 ref）都会先执行 cleanup，
  // 若只在初始化 useRef(true) 里赋值，ref 会永久停留在 false，
  // 导致 handleCitationClick 在挂载守卫处静默 return（点击引用无任何反应）。
  const isMountedRef = useRef(true);
  useEffect(() => {
    isMountedRef.current = true;
    return () => {
      isMountedRef.current = false;
    };
  }, []);

  // ChatPage 可能是用户进入 App 的第一个路由页（直接导航 / 链接跳 / 刷新），
  // fileStore.files 初始化是 []，本地精确匹配永远失败；在此兜底拉一次全量列表。
  // 列表本身用 getState 读取（不订阅，避免流式外的大数组订阅）。ref 防重入：
  // 若 listAllFiles 返回空列表，isScanning true→false 反复变化会让 effect 重跑，
  // 门闩防止无限 loadAllFiles 循环（真实环境表现为 IPC 刷屏 + 页面反复重渲染）。
  const bootstrappedRef = useRef(false);
  useEffect(() => {
    if (bootstrappedRef.current || !filesReady || useFileStore.getState().files.length > 0) {
      return;
    }
    bootstrappedRef.current = true;
    void loadAllFiles().catch(() => {
      // 拉取失败时解除门闩，允许下次依赖变化时重试；点击引用另有兜底加载路径
      bootstrappedRef.current = false;
    });
  }, [filesReady, loadAllFiles]);

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

  // 引用点击（async 主逻辑）。useCallback 稳定引用是 ChatBubble memo 生效的前提：
  // 内部只依赖 getState 读取与稳定 action，无每渲染变化的闭包值。
  const handleCitationClick = useCallback(
    async (citation: ChatCitation) => {
      // 最外层兜底：任何 throw 都转为 error toast——异步 unhandled rejection 会让用户以为"点了没反应"。
      try {
        const toastState = useToastStore.getState();
        const showToast = toastState.show;
        const removeToast = toastState.remove;

        if (!citation || typeof citation.fileName !== 'string' || citation.fileName.length === 0) {
          showToast({ message: '引用文件名为空，请检查回答内容格式', variant: 'error' });
          return;
        }

        // ① 是否需要"等待中"toast：只要要走 IPC 慢路径才显示
        let localFiles = useFileStore.getState().files;
        const needSlowPath = localFiles.length === 0;
        let loadingToastId: string | null = null;
        if (needSlowPath) {
          loadingToastId = showToast({
            message: `正在定位「${citation.fileName}」…`,
            variant: 'info',
            duration: 0,
          });
        }

        // ② 如需补全文件列表，主动 await（不再依赖 filesReady 门槛）
        if (localFiles.length === 0) {
          try {
            await loadAllFiles();
          } catch {
            if (loadingToastId) removeToast(loadingToastId);
            showToast({ message: '文件列表加载失败，稍后重试', variant: 'error' });
            return;
          }
          if (!isMountedRef.current) {
            if (loadingToastId) removeToast(loadingToastId);
            return;
          }
          localFiles = useFileStore.getState().files;
        }

        // ③ findFileByName 返回 {file, fastPath}：fastPath=true=本地前 3 级命中（<1ms）
        const found = await findFileByName(citation.fileName, localFiles);
        if (!isMountedRef.current) {
          if (loadingToastId) removeToast(loadingToastId);
          return;
        }

        if (!found) {
          if (loadingToastId) removeToast(loadingToastId);
          showToast({
            message: `未找到引用文件「${citation.fileName}」，请先在文件管理中扫描该目录`,
            variant: 'warn',
            duration: 4500,
          });
          return;
        }

        // ④ 命中：IPC 慢路径 → 补 loading toast
        if (!found.fastPath && !loadingToastId) {
          loadingToastId = showToast({
            message: `正在加载「${citation.fileName}」预览…`,
            variant: 'info',
            duration: 0,
          });
        }

        // ⑤ 一步到位：setPreviewKey(k+1)（强制卸旧实例清理 PDF 缓存）
        //    + setPreviewTarget(file)（React 批处理一次渲染）
        setPreviewKey((k) => k + 1);
        setPreviewTarget({ file: found.file, initialPage: citation.page });
        if (loadingToastId) removeToast(loadingToastId);
      } catch (err) {
        const message = err instanceof Error ? err.message : String(err);
        useToastStore.getState().show({
          message: `打开预览失败：${message}`,
          variant: 'error',
          duration: 5000,
        });
      }
    },
    [loadAllFiles],
  );

  const onCitationClick = useCallback(
    (citation: ChatCitation) => {
      void handleCitationClick(citation);
    },
    [handleCitationClick],
  );

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
            disabled={building || !engineReady}
            title={engineReady ? undefined : 'AI 引擎未就绪，暂时无法建立索引'}
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
            <ChatMessageList onCitationClick={onCitationClick} />
          )}

          <SearchStatusBar />
        </div>

        <LazyFilePreviewDrawer
          key={previewKey}
          file={previewTarget?.file ?? null}
          {...(previewTarget ? { initialPage: previewTarget.initialPage } : {})}
          onClose={() => setPreviewTarget(null)}
        />
      </div>

      <div className="chat-input-area">
        {!engineReady && (
          <div className="chat-page__engine-hint" role="status">
            {engineFailed ? (
              <>
                <span>AI 引擎启动失败，暂时无法提问。</span>
                <button
                  type="button"
                  className="status-bar__link"
                  onClick={() => void retrySidecar()}
                >
                  重试
                </button>
              </>
            ) : (
              <span>AI 引擎启动中，就绪后可开始问答…</span>
            )}
          </div>
        )}
        <ChatInput
          disabled={isStreaming || !engineReady}
          onSend={(content) => void sendMessage(content)}
        />
      </div>
    </div>
  );
}
