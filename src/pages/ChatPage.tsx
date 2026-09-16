// 知识问答页（设计稿 §6 / T6.6）：对话气泡 + 流式光标 + 引用标签 + 检索状态栏。
//
// 本组件只做编排：订阅低频 state（messages/isStreaming/error 等），把状态与动作
// 分发给头部、消息区与输入区；引用跳转的流程与过程提示收在 useCitationPreview。
//
// 渲染性能：token 级高频 state（currentStream/status/searchInfo 等）由
// ChatMessageList 内部 StreamingBubble 与 SearchStatusBar 自行订阅，
// 流式输出不整页重渲。

import { useEffect, useRef, useState } from 'react';

import { ChatHeader } from '@/components/chat/ChatHeader';
import { ChatInputArea } from '@/components/chat/ChatInputArea';
import { ChatMessageList } from '@/components/chat/ChatMessageList';
import { SearchStatusBar } from '@/components/chat/SearchStatusBar';
import { LazyFilePreviewDrawer } from '@/components/common/LazyFilePreviewDrawer';
import { useCitationPreview } from '@/hooks/useCitationPreview';
import { useHotkeys } from '@/hooks/useHotkeys';
import { fileIpc } from '@/lib/ipc';
import { useChatStore } from '@/stores/chatStore';
import { useFileStore } from '@/stores/fileStore';
import { useSidecarStore } from '@/stores/sidecarStore';

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

  // 建立索引：请求状态 + 结果提示（T7.x）
  const [building, setBuilding] = useState(false);
  const [indexMessage, setIndexMessage] = useState<string | null>(null);
  // 引用跳转：定位文件、过程提示、预览抽屉的挂载 key 与目标都在 hook 内
  const citation = useCitationPreview();

  // FE-m14：await 后 setState 的卸载守卫，防组件卸载后 setState warning。
  // 注意必须在 setup 里显式置 true：StrictMode（dev 双跑 setup→cleanup→setup）
  // 和 Vite HMR Fast Refresh（重跑 effect 但保留 ref）都会先执行 cleanup，
  // 若只在初始化 useRef(true) 里赋值，ref 会永久停留在 false。
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

  return (
    <div className="page chat-page">
      <ChatHeader
        totalFiles={totalFiles}
        showClearHistory={messages.length > 0}
        index={{
          building,
          message: indexMessage,
          ready: engineReady,
          onBuild: () => void handleBuildIndex(),
        }}
        onClearHistory={clearHistory}
      />

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
            <ChatMessageList onCitationClick={citation.openCitation} />
          )}

          <SearchStatusBar />
        </div>

        <LazyFilePreviewDrawer
          key={citation.mountKey}
          file={citation.target?.file ?? null}
          {...(citation.target ? { initialPage: citation.target.initialPage } : {})}
          onClose={citation.close}
        />
      </div>

      <ChatInputArea
        engineReady={engineReady}
        engineFailed={engineFailed}
        streaming={isStreaming}
        onRetryEngine={() => void retrySidecar()}
        onSend={(content) => void sendMessage(content)}
      />
    </div>
  );
}
