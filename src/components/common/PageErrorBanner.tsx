// 页面级错误横幅：错误文案 + 关闭按钮（各页面共用）。
//
// 各页面的样式类不同（files-page__error / rules-page__error / classify-page__error），
// 由调用方传入；DOM 结构与此前各页面内联的写法保持一致（容器 role="alert" +
// 文案 span + aria-label 关闭按钮），页面函数只留一行编排。

interface PageErrorBannerProps {
  /** 错误文案；null / 空串不渲染（等价于此前的 `{error && …}`） */
  message: string | null;
  /** 外层容器 class（含页面前缀，如 files-page__error） */
  className: string;
  /** 关闭按钮 class（如 files-page__error-dismiss） */
  dismissClassName: string;
  /** 关闭提示（清空 store.error） */
  onDismiss: () => void;
}

export function PageErrorBanner({
  message,
  className,
  dismissClassName,
  onDismiss,
}: PageErrorBannerProps) {
  if (!message) return null;
  return (
    <div className={className} role="alert">
      <span>{message}</span>
      <button
        type="button"
        className={dismissClassName}
        aria-label="关闭错误提示"
        onClick={onDismiss}
      >
        ×
      </button>
    </div>
  );
}
