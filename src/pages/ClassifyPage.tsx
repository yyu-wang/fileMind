// 智能分类页（对齐交互原型 §智能分类）：预览（左树 + 右统计）→ 分块执行 → 结果。
//
// 状态机渲染：
//   Idle（preview=null）→ 说明 + 开始分类按钮
//   Previewing → 加载提示
//   Preview（Idle + preview）→ 双栏：ClassifyPreviewTree + ClassifyStatsPanel
//   Running / Paused → 进度遮罩（覆盖在预览上）
//   Done / Cancelled → ClassifyDonePanel

import { useEffect, useState } from 'react';

import { ClassifyBatchDetail } from '@/components/classify/ClassifyBatchDetail';
import { ClassifyDonePanel } from '@/components/classify/ClassifyDonePanel';
import { ClassifyHistoryList } from '@/components/classify/ClassifyHistoryList';
import { ClassifyModeDialog } from '@/components/classify/ClassifyModeDialog';
import { ClassifyPreviewTree } from '@/components/classify/ClassifyPreviewTree';
import { ClassifyProgressOverlay } from '@/components/classify/ClassifyProgressOverlay';
import { ClassifyStatsPanel } from '@/components/classify/ClassifyStatsPanel';
import { FilePreviewDrawer, type FilePreviewTarget } from '@/components/common/FilePreviewDrawer';
import { ConfirmDialog } from '@/components/ui/ConfirmDialog';
import { isOrganized } from '@/lib/fileTable';
import { useClassifyHistoryStore } from '@/stores/classifyHistoryStore';
import { useClassifyStore, type ClassifyExecMode } from '@/stores/classifyStore';
import { useFileStore } from '@/stores/fileStore';
import { ClassifyStatus } from '@/types/models';
import type { OperationBatchSummary } from '@/types/ipc';

export function ClassifyPage() {
  const files = useFileStore((s) => s.files);
  const scanPath = useFileStore((s) => s.scanPath);
  const selectedIds = useFileStore((s) => s.selectedIds);

  const status = useClassifyStore((s) => s.status);
  const preview = useClassifyStore((s) => s.preview);
  const progress = useClassifyStore((s) => s.progress);
  const execSummary = useClassifyStore((s) => s.execSummary);
  const lastBatchId = useClassifyStore((s) => s.lastBatchId);
  const error = useClassifyStore((s) => s.error);
  const generatePreview = useClassifyStore((s) => s.generatePreview);
  const execute = useClassifyStore((s) => s.execute);
  const pause = useClassifyStore((s) => s.pause);
  const resume = useClassifyStore((s) => s.resume);
  const cancel = useClassifyStore((s) => s.cancel);
  const undoLastBatch = useClassifyStore((s) => s.undoLastBatch);
  const reset = useClassifyStore((s) => s.reset);
  const clearError = useClassifyStore((s) => s.clearError);

  // 「分类历史」视图状态：是否进入历史视图 + 待撤销批次的二次确认对象
  const [showHistory, setShowHistory] = useState(false);
  const [pendingUndo, setPendingUndo] = useState<OperationBatchSummary | null>(null);
  // 「分类方式」选择弹窗状态：确认执行时先选移动/复制，再按该模式执行
  const [pendingExecute, setPendingExecute] = useState<{ resolveConflicts: boolean } | null>(null);
  // 预览抽屉目标：点击树中文件名后打开对应文件预览（复用公共 FilePreviewDrawer）
  const [previewTarget, setPreviewTarget] = useState<FilePreviewTarget | null>(null);

  const historyBatches = useClassifyHistoryStore((s) => s.batches);
  const historyDetail = useClassifyHistoryStore((s) => s.detail);
  const historyLoading = useClassifyHistoryStore((s) => s.loading);
  const historyUndoing = useClassifyHistoryStore((s) => s.undoing);
  const historyError = useClassifyHistoryStore((s) => s.error);
  const loadHistory = useClassifyHistoryStore((s) => s.loadHistory);
  const openBatch = useClassifyHistoryStore((s) => s.openBatch);
  const closeBatch = useClassifyHistoryStore((s) => s.closeBatch);
  const undoBatch = useClassifyHistoryStore((s) => s.undoBatch);

  // 从文件页「整理选中」跳转进入：带选中态时自动生成预览（仅首次挂载）
  useEffect(() => {
    const { selectedIds } = useFileStore.getState();
    const { status: st, preview: prev, generatePreview: gen } = useClassifyStore.getState();
    if (selectedIds.length > 0 && st === ClassifyStatus.Idle && prev === null) {
      void gen(selectedIds);
    }
  }, []);

  const hasFiles = files.length > 0;
  const hasSelection = selectedIds.length > 0;
  // 软排除：无选中时「全部分类」只针对未整理文件；全部已整理则禁用并提示
  const unorganizedFiles = files.filter((f) => !isOrganized(f));
  const noUnorganizedTargets = hasFiles && !hasSelection && unorganizedFiles.length === 0;
  const showPreview =
    preview !== null && status !== ClassifyStatus.Done && status !== ClassifyStatus.Cancelled;

  // 底部警告条计数（对齐原型「⚠️ N 个冲突文件需处理 · M 个待确认」）
  const conflictCount = preview?.items.filter((i) => i.status === 'Conflict').length ?? 0;
  const pendingCount = preview?.stats.pending ?? 0;
  const showWarning = showPreview && (conflictCount > 0 || pendingCount > 0);

  const handleStart = () => {
    // 软排除：有选中尊重选中（可手动重选已整理文件）；无选中仅处理未整理文件
    const ids = hasSelection ? selectedIds : unorganizedFiles.map((f) => f.id);
    if (ids.length === 0) {
      // 防御：全部已整理且无选中（按钮已禁用，正常不触发），提示而非发空请求
      useClassifyStore.setState({ error: '当前没有未整理的文件，无需分类' });
      return;
    }
    void generatePreview(ids);
  };

  const handleOpenHistory = () => {
    setShowHistory(true);
    void loadHistory();
  };

  const handleConfirmUndo = async () => {
    if (!pendingUndo) return;
    await undoBatch(pendingUndo.batch_id);
    setPendingUndo(null);
  };

  const handleExecuteClick = (resolveConflicts: boolean) => {
    // 先弹「移动/复制」选择框，确认后再按所选模式执行
    setPendingExecute({ resolveConflicts });
  };

  const handleModeChosen = (mode: ClassifyExecMode) => {
    if (!pendingExecute) return;
    const { resolveConflicts } = pendingExecute;
    setPendingExecute(null);
    void execute(resolveConflicts, mode);
  };

  return (
    <div className="classify-page">
      <header className="classify-page__header">
        <div className="classify-page__heading">
          <h1 className="classify-page__title">智能分类</h1>
          {showPreview && <span className="classify-page__subtitle">预览分类方案</span>}
          {scanPath && (
            <span className="classify-page__path" title={scanPath}>
              {scanPath}
            </span>
          )}
        </div>
        {showPreview && (
          <div className="classify-page__actions">
            <button type="button" className="btn btn--ghost" onClick={reset}>
              取消
            </button>
            <button type="button" className="btn" onClick={() => handleExecuteClick(false)}>
              仅执行无冲突项
            </button>
            <button
              type="button"
              className="btn btn--primary"
              onClick={() => handleExecuteClick(true)}
            >
              ✓ 确认执行全部
            </button>
          </div>
        )}
      </header>

      {error && (
        <div className="classify-page__error" role="alert">
          <span>{error}</span>
          <button
            type="button"
            className="classify-page__error-dismiss"
            aria-label="关闭错误提示"
            onClick={clearError}
          >
            ×
          </button>
        </div>
      )}

      {status === ClassifyStatus.Previewing && (
        <div className="classify-page__loading" role="status">
          正在生成分类预览…
        </div>
      )}

      {showPreview && preview && (
        <div className="classify-main">
          {/* 左：树形分类预览（按钮在头部，此处仅展示） */}
          <div className="classify-main__tree">
            <ClassifyPreviewTree
              preview={preview}
              onOpenPreview={(item) =>
                setPreviewTarget({
                  path: item.original_path,
                  file_name: item.file_name,
                  category: item.category_name,
                })
              }
            />
          </div>
          {/* 右：统计面板 */}
          <ClassifyStatsPanel preview={preview} />
          {/* 右：文件预览抽屉（点击树中文件名打开；key 保证切换文件时重挂载重置状态） */}
          <FilePreviewDrawer
            key={previewTarget?.path ?? 'none'}
            file={previewTarget}
            onClose={() => setPreviewTarget(null)}
          />
          {(status === ClassifyStatus.Running || status === ClassifyStatus.Paused) && (
            <ClassifyProgressOverlay
              progress={progress}
              status={status}
              onPause={pause}
              onResume={resume}
              onCancel={cancel}
            />
          )}
        </div>
      )}

      {/* 底部警告条（对齐原型：仅保留提示文字，按钮收敛到头部一处） */}
      {showWarning && (
        <div className="classify-footer">
          <span className="warning-text">
            ⚠️ {conflictCount} 个冲突文件需处理 · {pendingCount} 个待确认
          </span>
        </div>
      )}

      {(status === ClassifyStatus.Done || status === ClassifyStatus.Cancelled) && execSummary && (
        <ClassifyDonePanel
          summary={execSummary}
          cancelled={status === ClassifyStatus.Cancelled}
          canUndo={lastBatchId != null}
          onUndo={() => void undoLastBatch()}
          onFinish={reset}
        />
      )}

      {status === ClassifyStatus.Idle &&
        !preview &&
        (showHistory ? (
          historyDetail ? (
            <ClassifyBatchDetail logs={historyDetail.logs} onBack={closeBatch} />
          ) : (
            <ClassifyHistoryList
              batches={historyBatches}
              loading={historyLoading}
              error={historyError}
              undoing={historyUndoing}
              onRefresh={() => void loadHistory()}
              onBack={() => setShowHistory(false)}
              onOpenBatch={(batchId) => void openBatch(batchId)}
              onRequestUndo={setPendingUndo}
            />
          )
        ) : (
          <div className="classify-page__intro">
            <p className="classify-page__intro-title">按规则与文件类型自动整理</p>
            <p className="classify-page__intro-sub">
              预览分类结果后执行；规则与类型识别均未命中的文件会进入「待确认」列表。
            </p>
            <button
              type="button"
              className="btn btn--primary"
              onClick={handleStart}
              disabled={!hasFiles || noUnorganizedTargets}
            >
              {hasSelection
                ? `对选中的 ${selectedIds.length} 个文件开始分类`
                : noUnorganizedTargets
                  ? '没有未整理的文件'
                  : hasFiles
                    ? '全部分类'
                    : '请先在文件页扫描目录'}
            </button>
            <button
              type="button"
              className="btn btn--ghost classify-page__history-btn"
              onClick={handleOpenHistory}
            >
              查看分类历史
            </button>
          </div>
        ))}

      {pendingUndo && (
        <ConfirmDialog
          title="撤销该批次？"
          message={`将把该批次 ${pendingUndo.total_count} 个文件恢复到整理前的位置（原路径已被占用等会撤销失败）。`}
          confirmLabel="确认撤销"
          danger
          loading={historyUndoing}
          onConfirm={() => void handleConfirmUndo()}
          onCancel={() => setPendingUndo(null)}
        />
      )}

      {pendingExecute && (
        <ClassifyModeDialog
          onChoose={(mode) => void handleModeChosen(mode)}
          onCancel={() => setPendingExecute(null)}
        />
      )}
    </div>
  );
}
