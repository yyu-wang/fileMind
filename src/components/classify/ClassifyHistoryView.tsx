// 分类历史视图：列表 ↔ 批次详情切换，并负责「撤销该批次」的二次确认。
//
// 自订阅 classifyHistoryStore：历史数据与撤销动作只被本视图消费，放这里可让页面
// 不持有 history 相关状态与 selector——页面只控制挂载/卸载，挂载即拉取列表。

import { useEffect, useState } from 'react';

import { ConfirmDialog } from '@/components/ui/ConfirmDialog';
import { useClassifyHistoryStore } from '@/stores/classifyHistoryStore';
import type { OperationBatchSummary } from '@/types/ipc';

import { ClassifyBatchDetail } from './ClassifyBatchDetail';
import { ClassifyHistoryList } from './ClassifyHistoryList';

interface ClassifyHistoryViewProps {
  /** 关闭历史视图（回到分类页初始态） */
  onBack: () => void;
}

export function ClassifyHistoryView({ onBack }: ClassifyHistoryViewProps) {
  const batches = useClassifyHistoryStore((s) => s.batches);
  const detail = useClassifyHistoryStore((s) => s.detail);
  const loading = useClassifyHistoryStore((s) => s.loading);
  const undoing = useClassifyHistoryStore((s) => s.undoing);
  const error = useClassifyHistoryStore((s) => s.error);
  const loadHistory = useClassifyHistoryStore((s) => s.loadHistory);
  const openBatch = useClassifyHistoryStore((s) => s.openBatch);
  const closeBatch = useClassifyHistoryStore((s) => s.closeBatch);
  const undoBatch = useClassifyHistoryStore((s) => s.undoBatch);

  /** 待撤销批次的二次确认对象（确认后才真正调用 undoBatch） */
  const [pendingUndo, setPendingUndo] = useState<OperationBatchSummary | null>(null);

  // 挂载即拉取：本视图只在「进入历史」时挂载，等价于原先点击「查看分类历史」时拉取
  useEffect(() => {
    void loadHistory();
  }, [loadHistory]);

  const handleConfirmUndo = async () => {
    if (!pendingUndo) return;
    await undoBatch(pendingUndo.batch_id);
    setPendingUndo(null);
  };

  if (detail) {
    return <ClassifyBatchDetail logs={detail.logs} onBack={closeBatch} />;
  }

  return (
    <>
      <ClassifyHistoryList
        batches={batches}
        loading={loading}
        error={error}
        undoing={undoing}
        onRefresh={() => void loadHistory()}
        onBack={onBack}
        onOpenBatch={(batchId) => void openBatch(batchId)}
        onRequestUndo={setPendingUndo}
      />
      {pendingUndo && (
        <ConfirmDialog
          title="撤销该批次？"
          message={`将把该批次 ${pendingUndo.total_count} 个文件恢复到整理前的位置（原路径已被占用等会撤销失败）。`}
          confirmLabel="确认撤销"
          danger
          loading={undoing}
          onConfirm={() => void handleConfirmUndo()}
          onCancel={() => setPendingUndo(null)}
        />
      )}
    </>
  );
}
