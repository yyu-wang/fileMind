// 智能分类页（对齐交互原型 §智能分类）：预览（左树 + 右统计）→ 分块执行 → 结果。
//
// 状态机渲染：
//   Idle（preview=null）→ 说明 + 开始分类按钮
//   Previewing → 加载提示
//   Preview（Idle + preview）→ 双栏：ClassifyPreviewTree + ClassifyStatsPanel
//   Running / Paused → 进度遮罩（覆盖在预览上）
//   Done / Cancelled → ClassifyDonePanel

import { useEffect } from 'react';

import { ClassifyDonePanel } from '@/components/classify/ClassifyDonePanel';
import { ClassifyPreviewTree } from '@/components/classify/ClassifyPreviewTree';
import { ClassifyProgressOverlay } from '@/components/classify/ClassifyProgressOverlay';
import { ClassifyStatsPanel } from '@/components/classify/ClassifyStatsPanel';
import { useClassifyStore } from '@/stores/classifyStore';
import { useFileStore } from '@/stores/fileStore';
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
  const showPreview =
    preview !== null && status !== ClassifyStatus.Done && status !== ClassifyStatus.Cancelled;

  // 底部警告条计数（对齐原型「⚠️ N 个冲突文件需处理 · M 个待确认」）
  const conflictCount = preview?.items.filter((i) => i.status === 'Conflict').length ?? 0;
  const pendingCount = preview?.stats.pending ?? 0;
  const showWarning = showPreview && (conflictCount > 0 || pendingCount > 0);

  const handleStart = () => {
    const ids = hasSelection ? selectedIds : files.map((f) => f.id);
    void generatePreview(ids);
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
            <button type="button" className="btn" onClick={() => void execute(false)}>
              仅执行无冲突项
            </button>
            <button type="button" className="btn btn--primary" onClick={() => void execute(true)}>
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
            <ClassifyPreviewTree preview={preview} />
          </div>
          {/* 右：统计面板 */}
          <ClassifyStatsPanel preview={preview} />
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

      {status === ClassifyStatus.Idle && !preview && (
        <div className="classify-page__intro">
          <p className="classify-page__intro-title">按规则与文件类型自动整理</p>
          <p className="classify-page__intro-sub">
            预览分类结果后执行；规则与类型识别均未命中的文件会进入「待确认」列表。
          </p>
          <button
            type="button"
            className="btn btn--primary"
            onClick={handleStart}
            disabled={!hasFiles}
          >
            {hasSelection
              ? `对选中的 ${selectedIds.length} 个文件开始分类`
              : hasFiles
                ? '全部分类'
                : '请先在文件页扫描目录'}
          </button>
        </div>
      )}
    </div>
  );
}
