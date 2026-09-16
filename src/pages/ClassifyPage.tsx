// 智能分类页（对齐交互原型 §智能分类）：预览（左树 + 右统计）→ 分块执行 → 结果。
//
// 状态机渲染（各区域显隐的判定收在 lib/classifyView.ts，本组件只做编排）：
//   Idle（preview=null）→ ClassifyIntroPanel 或 ClassifyHistoryView
//   Previewing → 加载提示
//   Preview（Idle + preview）→ ClassifyPreviewSection
//   Running / Paused → 同上 + 进度遮罩
//   Done / Cancelled → ClassifyDonePanel

import { useEffect, useState } from 'react';

import { ClassifyDonePanel } from '@/components/classify/ClassifyDonePanel';
import { ClassifyHeader } from '@/components/classify/ClassifyHeader';
import { ClassifyHistoryView } from '@/components/classify/ClassifyHistoryView';
import { ClassifyIntroPanel } from '@/components/classify/ClassifyIntroPanel';
import { ClassifyModeDialog } from '@/components/classify/ClassifyModeDialog';
import { ClassifyPreviewSection } from '@/components/classify/ClassifyPreviewSection';
import type { FilePreviewTarget } from '@/components/common/FilePreviewDrawer';
import { resolveClassifyPageView } from '@/lib/classifyView';
import { isOrganized } from '@/lib/fileTable';
import { useClassifyStore, type ClassifyExecMode } from '@/stores/classifyStore';
import { useFileStore } from '@/stores/fileStore';
import type { ClassifyPlanItem } from '@/types/ipc';
import { ClassifyStatus } from '@/types/models';

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

  /** 是否进入「分类历史」视图；「分类方式」弹窗的待执行参数；预览抽屉目标 */
  const [historyOpen, setHistoryOpen] = useState(false);
  const [pendingExecute, setPendingExecute] = useState<{ resolveConflicts: boolean } | null>(null);
  const [previewTarget, setPreviewTarget] = useState<FilePreviewTarget | null>(null);

  // 从文件页「整理选中」跳转进入：带选中态时自动生成预览（仅首次挂载）
  useEffect(() => {
    const { selectedIds: ids } = useFileStore.getState();
    const { status: st, preview: prev, generatePreview: gen } = useClassifyStore.getState();
    if (ids.length > 0 && st === ClassifyStatus.Idle && prev === null) {
      void gen(ids);
    }
  }, []);

  const hasSelection = selectedIds.length > 0;
  const unorganizedFiles = files.filter((f) => !isOrganized(f));
  const view = resolveClassifyPageView({
    status,
    preview,
    execSummary,
    hasUndoBatch: lastBatchId !== null,
    fileCounts: {
      total: files.length,
      selected: selectedIds.length,
      unorganized: unorganizedFiles.length,
    },
  });

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

  const handleCancelPreview = () => {
    // FE-M9：取消丢弃预览时同步清空文件页选中——否则残留的
    // selectedIds 在下次进入分类页时又触发自动生成
    useFileStore.getState().clearSelection();
    reset();
  };

  const handleModeChosen = (mode: ClassifyExecMode) => {
    if (!pendingExecute) return;
    const { resolveConflicts } = pendingExecute;
    setPendingExecute(null);
    void execute(resolveConflicts, mode);
  };

  const handleOpenPreviewItem = (item: ClassifyPlanItem) => {
    setPreviewTarget({
      path: item.original_path,
      file_name: item.file_name,
      category: item.category_name,
    });
  };

  return (
    <div className="page classify-page">
      <ClassifyHeader
        scanPath={scanPath}
        showPreviewSubtitle={view.showPreviewSubtitle}
        showActions={view.showHeaderActions}
        onCancelPreview={handleCancelPreview}
        onExecute={(resolveConflicts) => setPendingExecute({ resolveConflicts })}
      />

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

      {view.loading && (
        <div className="classify-page__loading" role="status">
          正在生成分类预览…
        </div>
      )}

      {view.layoutPreview && (
        <ClassifyPreviewSection
          preview={view.layoutPreview}
          status={status}
          progress={progress}
          drawer={{ target: previewTarget, onClose: () => setPreviewTarget(null) }}
          actions={{
            onOpenPreview: handleOpenPreviewItem,
            onPause: pause,
            onResume: resume,
            onCancel: cancel,
          }}
        />
      )}

      {/* 底部警告条（对齐原型：仅保留提示文字，按钮收敛到头部一处） */}
      {view.showWarning && (
        <div className="classify-footer">
          <span className="warning-text">
            ⚠️ {view.conflictCount} 个冲突文件需处理 · {view.pendingCount} 个待确认
          </span>
        </div>
      )}

      {view.doneSummary && (
        <ClassifyDonePanel
          summary={view.doneSummary}
          cancelled={view.cancelled}
          canUndo={view.canUndo}
          onUndo={() => void undoLastBatch()}
          onFinish={reset}
        />
      )}

      {view.idle &&
        (historyOpen ? (
          <ClassifyHistoryView onBack={() => setHistoryOpen(false)} />
        ) : (
          <ClassifyIntroPanel
            startButton={view.startButton}
            onStart={handleStart}
            onOpenHistory={() => setHistoryOpen(true)}
          />
        ))}

      {pendingExecute && (
        <ClassifyModeDialog
          onChoose={(mode) => void handleModeChosen(mode)}
          onCancel={() => setPendingExecute(null)}
        />
      )}
    </div>
  );
}
