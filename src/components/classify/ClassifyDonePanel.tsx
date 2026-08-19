// 分类结果面板：执行完成/取消后的汇总 + 撤销整批 + 完成（设计稿 §5.3）。

import type { ClassifyExecSummary } from '@/stores/classifyStore';

interface ClassifyDonePanelProps {
  /** 执行结果汇总 */
  summary: ClassifyExecSummary;
  /** 本次是否被用户取消（展示「部分执行」提示） */
  cancelled: boolean;
  /** 撤销最近整批 */
  onUndo: () => void;
  /** 完成（回到分类页初始态） */
  onFinish: () => void;
}

export function ClassifyDonePanel({
  summary,
  cancelled,
  onUndo,
  onFinish,
}: ClassifyDonePanelProps) {
  const unprocessed = Math.max(
    summary.total - summary.success - summary.failed - summary.pending,
    0,
  );

  return (
    <div className="classify-done">
      <h3 className="classify-done__title">
        {cancelled ? '分类已取消（部分文件已处理）' : '分类完成'}
      </h3>
      <ul className="classify-done__stats">
        <li>
          <span className="classify-done__num classify-done__num--success">{summary.success}</span>
          <span className="classify-done__label">成功</span>
        </li>
        <li>
          <span className="classify-done__num classify-done__num--failed">{summary.failed}</span>
          <span className="classify-done__label">失败</span>
        </li>
        <li>
          <span className="classify-done__num classify-done__num--pending">{summary.pending}</span>
          <span className="classify-done__label">待确认</span>
        </li>
        {unprocessed > 0 && (
          <li>
            <span className="classify-done__num classify-done__num--muted">{unprocessed}</span>
            <span className="classify-done__label">未处理</span>
          </li>
        )}
      </ul>
      <div className="classify-done__actions">
        {!cancelled && summary.success > 0 && (
          <button type="button" className="btn" onClick={onUndo}>
            撤销本批
          </button>
        )}
        <button type="button" className="btn btn--primary" onClick={onFinish}>
          完成
        </button>
      </div>
    </div>
  );
}
