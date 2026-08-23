// 通用确认对话框（覆盖式二次确认）：危险操作在真正执行前
// 展示标题 + 说明 + 确认/取消按钮，避免误触造成不可逆的文件操作。
//
// 弹层复用 settings-dialog 的居中遮罩样式；`danger` 时确认按钮红色强调。

interface ConfirmDialogProps {
  title: string;
  message: string;
  confirmLabel: string;
  cancelLabel?: string;
  danger?: boolean;
  loading?: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}

export function ConfirmDialog({
  title,
  message,
  confirmLabel,
  cancelLabel = '取消',
  danger = false,
  loading = false,
  onConfirm,
  onCancel,
}: ConfirmDialogProps) {
  return (
    <div
      className="dialog-backdrop"
      role="presentation"
      onMouseDown={(e) => {
        // 点击遮罩关闭；对话框内部点击不冒泡，避免误关
        if (e.target === e.currentTarget && !loading) {
          onCancel();
        }
      }}
    >
      <div className="dialog" role="dialog" aria-modal="true" aria-label={title}>
        <p className="dialog__title">{title}</p>
        <p className="dialog__message">{message}</p>
        <div className="dialog__actions">
          <button type="button" className="btn btn--ghost" onClick={onCancel} disabled={loading}>
            {cancelLabel}
          </button>
          <button
            type="button"
            className={danger ? 'btn btn--danger' : 'btn btn--primary'}
            onClick={onConfirm}
            disabled={loading}
          >
            {loading ? '处理中…' : confirmLabel}
          </button>
        </div>
      </div>
    </div>
  );
}
