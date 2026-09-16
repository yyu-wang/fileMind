// 知识问答页头部：标题、已索引文件数、建立索引与清空对话。
//
// 纯展示组件：状态与动作由页面注入。索引结果提示（index.message）是索引动作的
// 产物，因此也在这里渲染，紧跟在头部下方。

interface ChatHeaderProps {
  /** 已索引文件数（标题右侧展示） */
  totalFiles: number;
  /** 是否显示「清空对话」（有消息时才显示） */
  showClearHistory: boolean;
  /** 索引区块：执行中状态、结果提示、引擎是否就绪、触发建索引 */
  index: {
    building: boolean;
    message: string | null;
    ready: boolean;
    onBuild: () => void;
  };
  /** 清空当前会话 */
  onClearHistory: () => void;
}

export function ChatHeader({
  totalFiles,
  showClearHistory,
  index,
  onClearHistory,
}: ChatHeaderProps) {
  return (
    <>
      <header className="chat-header">
        <span className="title">💬 知识问答</span>
        <span className="meta">基于 {totalFiles} 个已索引文件</span>
        <div className="header-actions">
          {/* T7.x：建立索引（向量化入库 LanceDB，问答检索的数据源） */}
          <button
            type="button"
            className="btn btn--ghost btn--sm"
            data-testid="build-index"
            onClick={index.onBuild}
            disabled={index.building || !index.ready}
            title={index.ready ? undefined : 'AI 引擎未就绪，暂时无法建立索引'}
          >
            {index.building ? '索引中…' : '建立索引'}
          </button>
          {showClearHistory && (
            <button type="button" className="btn btn--ghost btn--sm" onClick={onClearHistory}>
              🧹 清空对话
            </button>
          )}
        </div>
      </header>

      {index.message && (
        <div className="chat-page__index-tip" role="status">
          <span>{index.message}</span>
        </div>
      )}
    </>
  );
}
