// Modal 通用弹窗组件（对齐交互原型 §Modal）。
//
// 结构：modal-overlay > modal > modal-header + modal-body + modal-footer
// 动画：overlay fadeIn / modal scaleIn（CSS 已定义）
// 交互：点击遮罩关闭（可通过 closeOnOverlayClick 禁用）、ESC 关闭
//
// 用法：
//   <Modal title="标题" onClose={handleClose} footer={<button>确定</button>}>
//     <p>内容</p>
//   </Modal>

import { useEffect, type MouseEvent, type ReactNode } from 'react';

interface ModalProps {
  /** 标题（显示在 modal-header） */
  title: string;
  /** 标题前的图标（可选） */
  icon?: ReactNode;
  /** body 内容 */
  children: ReactNode;
  /** footer 区域（通常放确认/取消按钮） */
  footer?: ReactNode;
  /** 关闭回调（点击遮罩或 ESC 时触发） */
  onClose?: () => void;
  /** 点击遮罩是否关闭（默认 true） */
  closeOnOverlayClick?: boolean;
  /** ESC 是否关闭（默认 true） */
  closeOnEsc?: boolean;
  /** 模态宽度（默认 500px） */
  maxWidth?: number;
}

export function Modal({
  title,
  icon,
  children,
  footer,
  onClose,
  closeOnOverlayClick = true,
  closeOnEsc = true,
  maxWidth = 500,
}: ModalProps) {
  useEffect(() => {
    if (!closeOnEsc) return;
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        onClose?.();
      }
    };
    document.addEventListener('keydown', handleKeyDown);
    return () => {
      document.removeEventListener('keydown', handleKeyDown);
    };
  }, [closeOnEsc, onClose]);

  const handleOverlayClick = (e: MouseEvent<HTMLDivElement>) => {
    if (closeOnOverlayClick && e.target === e.currentTarget) {
      onClose?.();
    }
  };

  return (
    <div className="modal-overlay" role="presentation" onMouseDown={handleOverlayClick}>
      <div
        className="modal"
        role="dialog"
        aria-modal="true"
        aria-label={title}
        style={{ maxWidth }}
      >
        <div className="modal-header">
          {icon}
          {title}
        </div>
        <div className="modal-body">{children}</div>
        {footer && <div className="modal-footer">{footer}</div>}
      </div>
    </div>
  );
}
