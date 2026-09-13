// 分类方式选择弹窗：确认执行前选择「移动」或「复制」。
//
// 移动：把文件移出源目录，落入同级收纳目录 `<源目录名>_已分类` 的分类子文件夹。
// 复制：保留原文件不动，复制一份副本到同级收纳目录的分类子文件夹。
// 复用 dialog 基类样式；选项为两张大按钮 + 取消。

import type { ClassifyExecMode } from '@/stores/classifyStore';

interface ClassifyModeDialogProps {
  onChoose: (mode: ClassifyExecMode) => void;
  onCancel: () => void;
}

export function ClassifyModeDialog({ onChoose, onCancel }: ClassifyModeDialogProps) {
  return (
    <div
      className="dialog-backdrop"
      role="presentation"
      onMouseDown={(e) => {
        // 点击遮罩关闭；对话框内部点击不冒泡，避免误关
        if (e.target === e.currentTarget) {
          onCancel();
        }
      }}
    >
      <div className="dialog" role="dialog" aria-modal="true" aria-label="选择分类方式">
        <p className="dialog__title">选择分类方式</p>
        <p className="dialog__message">
          文件会整理到源目录**同级**的收纳目录（如
          `~/文档_已分类/`），源目录保持只留未整理文件。两种方式都会对文件打上分类标签。
        </p>
        <div className="mode-dialog__options">
          <button
            type="button"
            className="mode-dialog__option"
            data-testid="classify-mode-move"
            onClick={() => onChoose('move')}
          >
            <span className="mode-dialog__option-title">移动分类</span>
            <span className="mode-dialog__option-desc">
              移出源目录到收纳目录的分类子文件夹（原文件不再留在原处）
            </span>
          </button>
          <button type="button" className="mode-dialog__option" onClick={() => onChoose('copy')}>
            <span className="mode-dialog__option-title">复制分类</span>
            <span className="mode-dialog__option-desc">
              保留原文件不动，复制一份到收纳目录的分类子文件夹
            </span>
          </button>
        </div>
        <div className="dialog__actions">
          <button type="button" className="btn btn--ghost" onClick={onCancel}>
            取消
          </button>
        </div>
      </div>
    </div>
  );
}
