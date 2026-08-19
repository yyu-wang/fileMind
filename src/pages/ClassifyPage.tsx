// 智能分类页（设计稿 §5）：预览 → 分块执行（进度遮罩）→ 结果/撤销。
//
// 状态机渲染：
//   Idle（preview=null）→ 说明 + 开始分类按钮
//   Previewing → 加载提示
//   Preview（Idle + preview）→ ClassifyPreviewTree
//   Running / Paused → 进度遮罩（覆盖在预览树上）
//   Done / Cancelled → ClassifyDonePanel

import { useEffect } from 'react';

import { ClassifyDonePanel } from '@/components/classify/ClassifyDonePanel';
import { ClassifyPreviewTree } from '@/components/classify/ClassifyPreviewTree';
import { ClassifyProgressOverlay } from '@/components/classify/ClassifyProgressOverlay';
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

  const handleStart = () => {
    const ids = hasSelection ? selectedIds : files.map((f) => f.id);
    void generatePreview(ids);
  };

  return (
    <div className="classify-page">
      <header className="classify-page__header">
        <h1 className="classify-page__title">智能分类</h1>
        {scanPath && (
          <span className="classify-page__path" title={scanPath}>
            {scanPath}
          </span>
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
        <>
          <ClassifyPreviewTree preview={preview} onExecute={() => void execute()} onReset={reset} />
          {(status === ClassifyStatus.Running || status === ClassifyStatus.Paused) && (
            <ClassifyProgressOverlay
              progress={progress}
              status={status}
              onPause={pause}
              onResume={resume}
              onCancel={cancel}
            />
          )}
        </>
      )}

      {(status === ClassifyStatus.Done || status === ClassifyStatus.Cancelled) && execSummary && (
        <ClassifyDonePanel
          summary={execSummary}
          cancelled={status === ClassifyStatus.Cancelled}
          onUndo={() => void undoLastBatch()}
          onFinish={reset}
        />
      )}

      {status === ClassifyStatus.Idle && !preview && (
        <div className="classify-page__intro">
          <p className="classify-page__intro-title">按规则与启发式自动整理文件</p>
          <p className="classify-page__intro-sub">
            预览分类结果后执行；规则与启发式均未命中的文件会进入「待确认」列表。
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
