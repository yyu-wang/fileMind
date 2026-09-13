// 分类历史批次明细：逐文件展示文件名、操作类型、状态与 源 → 目标 路径。
// 仅已执行成功（done）且目标文件仍存在的条目提供「预览」入口，复用公共抽屉。
// 失败记录不提供预览：其源/目标文件常已不在原路径，预览会因路径不存在而报错。

import { useState } from 'react';

import { LazyFilePreviewDrawer } from '@/components/common/LazyFilePreviewDrawer';
import type { FilePreviewTarget } from '@/components/common/FilePreviewDrawer';
import { formatDateTime } from '@/lib/format';
import type { OperationLog } from '@/types/ipc';

import { HistoryStatusTag } from './ClassifyHistoryStatusTag';

const OP_TYPE_LABELS: Record<string, string> = {
  move: '移动',
  rename: '重命名',
  delete: '删除',
  copy: '复制',
};

/** 成功后可安全预览「目标文件」的操作类型（delete 的目标已不在原路径）。 */
const PREVIEWABLE_OPS = new Set(['move', 'rename', 'copy']);

/** 从绝对路径取文件名（兼容 / 与 Windows \）。 */
function basename(path: string): string {
  return path.split(/[\\/]/).pop() ?? path;
}

/**
 * 解析单条操作日志的预览目标：仅成功（done）且目标文件仍在磁盘上时可预览。
 *
 * 失败记录不显示预览：源/目标路径常已失效（目录被移动/改名/删除），
 * 强行预览会命中路径校验「路径不存在」。返回 `null` 表示隐藏预览按钮。
 */
function previewTargetForLog(log: OperationLog): FilePreviewTarget | null {
  if (log.status !== 'done') {
    return null;
  }
  if (log.target_path.trim() === '' || !PREVIEWABLE_OPS.has(log.operation_type)) {
    return null;
  }
  return { path: log.target_path, file_name: basename(log.target_path) };
}

interface ClassifyBatchDetailProps {
  logs: OperationLog[];
  onBack: () => void;
}

export function ClassifyBatchDetail({ logs, onBack }: ClassifyBatchDetailProps) {
  // 预览抽屉目标：点击「预览」后打开对应文件（key 变化驱动抽屉重挂载）
  const [previewTarget, setPreviewTarget] = useState<FilePreviewTarget | null>(null);

  return (
    <div className="history-view">
      <div className="history-view__header">
        <div className="history-view__heading">
          <h2 className="history-view__title">批次明细</h2>
          <span className="history-view__sub">
            {formatDateTime(logs[0]?.created_at ?? '')} · {logs.length} 条操作
          </span>
        </div>
        <div className="history-view__actions">
          <button type="button" className="btn btn--ghost" onClick={onBack}>
            返回列表
          </button>
        </div>
      </div>

      <ul className="history-detail">
        {logs.map((log) => {
          const preview = previewTargetForLog(log);
          return (
            <li key={log.id} className="history-detail__item">
              <div className="history-detail__top">
                <span className="history-detail__name" title={log.source_path}>
                  {basename(log.source_path)}
                </span>
                <span className="history-detail__op">
                  {OP_TYPE_LABELS[log.operation_type] ?? log.operation_type}
                </span>
                <HistoryStatusTag status={log.status} />
                {preview && (
                  <button
                    type="button"
                    className="btn btn--ghost btn--sm history-detail__preview"
                    onClick={() => setPreviewTarget(preview)}
                  >
                    预览
                  </button>
                )}
              </div>
              <div className="history-detail__paths">
                <span className="history-detail__path" title={log.source_path}>
                  {log.source_path}
                </span>
                <span className="history-detail__arrow" aria-hidden>
                  →
                </span>
                <span className="history-detail__path" title={log.target_path}>
                  {log.target_path}
                </span>
              </div>
            </li>
          );
        })}
      </ul>

      {/* 预览抽屉：复用文件页/分类预览树的统一预览组件（file 为 null 时不渲染） */}
      <LazyFilePreviewDrawer
        key={previewTarget?.path ?? 'none'}
        file={previewTarget}
        onClose={() => setPreviewTarget(null)}
      />
    </div>
  );
}
