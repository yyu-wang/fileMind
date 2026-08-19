// 分类执行进度遮罩：半透明全屏 + 进度条 + 暂停/继续/取消（设计稿 §5.2）。
//
// 仅当 status 为 Running/Paused 时由父级渲染；Paused 态展示「继续」按钮。

import { ClassifyStatus } from '@/types/models';

interface ClassifyProgressOverlayProps {
  /** 当前进度 */
  progress: { done: number; total: number };
  /** 当前状态（Running / Paused） */
  status: ClassifyStatus;
  /** 暂停 */
  onPause: () => void;
  /** 继续 */
  onResume: () => void;
  /** 取消 */
  onCancel: () => void;
}

const DEFAULT_TOTAL = 1;

export function ClassifyProgressOverlay({
  progress,
  status,
  onPause,
  onResume,
  onCancel,
}: ClassifyProgressOverlayProps) {
  const total = progress.total > 0 ? progress.total : DEFAULT_TOTAL;
  const percent = Math.round((progress.done / total) * 100);
  const paused = status === ClassifyStatus.Paused;

  return (
    <div className="classify-progress" role="dialog" aria-label="分类执行进度">
      <div className="classify-progress__panel">
        <p className="classify-progress__title">
          {paused ? '已暂停' : '正在分类…'}
          <span className="classify-progress__count">
            {progress.done}/{progress.total}
          </span>
        </p>
        <div
          className="classify-progress__bar"
          role="progressbar"
          aria-valuenow={progress.done}
          aria-valuemin={0}
          aria-valuemax={progress.total}
        >
          <div className="classify-progress__fill" style={{ width: `${percent}%` }} />
        </div>
        <p className="classify-progress__percent">{percent}%</p>
        <div className="classify-progress__controls">
          {paused ? (
            <button type="button" className="btn btn--primary" onClick={onResume}>
              继续
            </button>
          ) : (
            <button type="button" className="btn" onClick={onPause}>
              暂停
            </button>
          )}
          <button type="button" className="btn btn--ghost" onClick={onCancel}>
            取消
          </button>
        </div>
      </div>
    </div>
  );
}
