// ProgressOverlay 通用进度遮罩组件（对齐交互原型 §Progress Overlay）。
//
// 结构：progress-overlay.open > progress-card
// 用途：文件整理、向量索引重建等长任务进度展示，全屏遮罩阻止用户操作。
//
// 用法：
//   <ProgressOverlay
//     open={open}
//     title="正在整理文件..."
//     total={1234}
//     current={567}
//     currentLabel="src/foo.txt"
//     ok={560}
//     fail={7}
//   />

interface ProgressOverlayProps {
  /** 是否显示（控制 .open 类） */
  open: boolean;
  /** 卡片标题（progress-card h3） */
  title: string;
  /** 总数（用于显示 0 / 1,234 和百分比） */
  total: number;
  /** 当前进度（已完成数） */
  current: number;
  /** 正在处理的文件名/路径（progress-current 行） */
  currentLabel?: string;
  /** 成功计数 */
  ok?: number;
  /** 失败计数 */
  fail?: number;
}

export function ProgressOverlay({
  open,
  title,
  total,
  current,
  currentLabel,
  ok = 0,
  fail = 0,
}: ProgressOverlayProps) {
  if (!open) return null;

  const percent = total > 0 ? Math.min(100, Math.floor((current / total) * 100)) : 0;

  return (
    <div className="progress-overlay open" role="dialog" aria-modal="true" aria-label={title}>
      <div className="progress-card">
        <h3>{title}</h3>
        <div className="progress-bar-wrap">
          <div className="progress-bar-fill" style={{ width: `${percent}%` }} />
        </div>
        <div className="progress-info">
          <span style={{ color: 'var(--muted)' }}>
            {current.toLocaleString()} / {total.toLocaleString()}
          </span>
          <span style={{ fontWeight: 600, color: 'var(--accent)' }}>{percent}%</span>
        </div>
        {currentLabel !== undefined && (
          <div className="progress-current" title={currentLabel}>
            {currentLabel}
          </div>
        )}
        <div className="progress-stats">
          <span className="ok">✓ 成功 {ok.toLocaleString()}</span>
          <span className="fail">⚠️ 失败 {fail.toLocaleString()}</span>
        </div>
      </div>
    </div>
  );
}
