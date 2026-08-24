// 分类方式选择弹窗：确认执行前选择「移动」或「复制」。
//
// 移动：把文件移入分类子文件夹（原文件离开原目录，现有行为）。
// 复制：保留原文件不动，复制一份副本到分类子文件夹（不影响原文件）。
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
          分类会自动创建子文件夹整理文件。两种方式都会对文件打上分类标签。
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
              把文件移入分类子文件夹（原文件离开原目录）
            </span>
          </button>
          <button type="button" className="mode-dialog__option" onClick={() => onChoose('copy')}>
            <span className="mode-dialog__option-title">复制分类</span>
            <span className="mode-dialog__option-desc">保留原文件不动，复制一份到分类子文件夹</span>
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
