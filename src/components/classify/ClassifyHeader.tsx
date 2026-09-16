// 智能分类页头部：标题 + 预览副标题 + 扫描路径 + 操作按钮组。
//
// 按钮组在执行中（Running/Paused）由父级隐藏：进度遮罩盖不住 header，隐藏即可
// 双保险阻止第二个 execute，并消除头部「取消」（丢弃预览）与遮罩内「取消」
// （保留已执行块）的语义冲突——执行期只留遮罩内一个取消入口（showHeaderActions）。

interface ClassifyHeaderProps {
  /** 当前扫描根路径（null 时不渲染） */
  scanPath: string | null;
  /** 是否显示「预览分类方案」副标题 */
  showPreviewSubtitle: boolean;
  /** 是否显示操作按钮组 */
  showActions: boolean;
  /** 取消并丢弃当前预览 */
  onCancelPreview: () => void;
  /** 请求执行：true = 确认执行全部（冲突项按重命名处理） */
  onExecute: (resolveConflicts: boolean) => void;
}

export function ClassifyHeader({
  scanPath,
  showPreviewSubtitle,
  showActions,
  onCancelPreview,
  onExecute,
}: ClassifyHeaderProps) {
  return (
    <header className="main-header">
      <h1>智能分类</h1>
      {showPreviewSubtitle && <span className="subtitle">预览分类方案</span>}
      {scanPath && (
        <span className="subtitle" title={scanPath}>
          {scanPath}
        </span>
      )}
      {showActions && (
        <div className="header-actions">
          <button type="button" className="btn btn--ghost btn--sm" onClick={onCancelPreview}>
            取消
          </button>
          <button type="button" className="btn btn--sm" onClick={() => onExecute(false)}>
            仅执行无冲突项
          </button>
          <button
            type="button"
            className="btn btn--primary btn--sm"
            data-testid="classify-execute"
            onClick={() => onExecute(true)}
          >
            ✓ 确认执行全部
          </button>
        </div>
      )}
    </header>
  );
}
