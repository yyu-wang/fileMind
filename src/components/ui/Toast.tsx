// Toast 通知系统（对齐交互原型 §Toast）。
//
// 结构：toast-container（固定右下角）> toast*（单条通知）
// 动画：toastIn 入场 / toastOut 离场（CSS 已定义）
// 用法：
//   // 在应用根节点挂载容器
//   <ToastContainer />
//
//   // 在任意组件中触发
//   import { useToastStore } from '@/components/ui/Toast';
//   const showToast = useToastStore((s) => s.show);
//   showToast({ message: '保存成功', variant: 'success' });

import { create } from 'zustand';

type ToastVariant = 'info' | 'success' | 'warn' | 'error';

interface ToastItem {
  id: string;
  message: string;
  variant: ToastVariant;
  actionLabel?: string;
  onAction?: () => void;
  /** 自动关闭延迟（ms），默认 3000；设 0 则不自动关闭 */
  duration?: number;
}

interface ToastStore {
  toasts: ToastItem[];
  show: (toast: Omit<ToastItem, 'id'>) => string;
  remove: (id: string) => void;
}

let toastId = 0;

export const useToastStore = create<ToastStore>((set, get) => ({
  toasts: [],
  show: (toast) => {
    const id = `toast-${++toastId}`;
    const item: ToastItem = { id, duration: 3000, ...toast };
    set((state) => ({ toasts: [...state.toasts, item] }));
    if (item.duration && item.duration > 0) {
      setTimeout(() => {
        get().remove(id);
      }, item.duration);
    }
    return id;
  },
  remove: (id) => {
    set((state) => ({
      toasts: state.toasts.map((t) => (t.id === id ? { ...t, id: `removing-${t.id}` } : t)),
    }));
    // 等动画完成后再真正移除
    setTimeout(() => {
      set((state) => ({
        toasts: state.toasts.filter((t) => t.id !== `removing-${id}`),
      }));
    }, 250);
  },
}));

const VARIANT_ICON: Record<ToastVariant, string> = {
  info: 'ℹ️',
  success: '✓',
  warn: '⚠️',
  error: '✗',
};

/** 单条 Toast 通知 */
function ToastToast({ toast }: { toast: ToastItem }) {
  const remove = useToastStore((s) => s.remove);
  const isRemoving = toast.id.startsWith('removing-');

  return (
    <div className={`toast ${toast.variant}${isRemoving ? ' removing' : ''}`} role="alert">
      <span className="toast-icon" aria-hidden>
        {VARIANT_ICON[toast.variant]}
      </span>
      <span>{toast.message}</span>
      {toast.actionLabel && toast.onAction && (
        <button
          type="button"
          className="toast-action"
          onClick={() => {
            toast.onAction?.();
            remove(toast.id.replace('removing-', ''));
          }}
        >
          {toast.actionLabel}
        </button>
      )}
    </div>
  );
}

/** Toast 容器：挂载在应用根节点，自动渲染所有活跃通知 */
export function ToastContainer() {
  const toasts = useToastStore((s) => s.toasts);

  if (toasts.length === 0) return null;

  return (
    <div className="toast-container" aria-live="polite" aria-atomic="false">
      {toasts.map((toast) => (
        <ToastToast key={toast.id} toast={toast} />
      ))}
    </div>
  );
}
