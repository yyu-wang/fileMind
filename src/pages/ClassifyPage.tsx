// 智能分类页（对齐交互原型 §智能分类）：预览（左树 + 右统计）→ 分块执行 → 结果。
//
// 状态机渲染：
//   Idle（preview=null）→ ClassifyIntroPanel 或 ClassifyHistoryView
//   Previewing → 加载提示
//   Preview（Idle + preview）→ ClassifyPreviewSection
//   Running / Paused → 同上 + 进度遮罩
//   Done / Cancelled → ClassifyDonePanel
//
// 页面函数只做编排：store 订阅与派生在 useClassifyPageController，各区域显隐判定在
// lib/classifyView.ts，区域本身是 components/classify/ 下的子组件。

import { ClassifyDonePanel } from '@/components/classify/ClassifyDonePanel';
import { ClassifyHeader } from '@/components/classify/ClassifyHeader';
import { ClassifyIdleView } from '@/components/classify/ClassifyIdleView';
import { ClassifyModeDialog } from '@/components/classify/ClassifyModeDialog';
import { ClassifyPreviewArea } from '@/components/classify/ClassifyPreviewArea';
import { PageErrorBanner } from '@/components/common/PageErrorBanner';
import { useClassifyPageController } from '@/hooks/useClassifyPageController';

export function ClassifyPage() {
  const page = useClassifyPageController();

  return (
    <div className="page classify-page">
      <ClassifyHeader
        scanPath={page.scanPath}
        showPreviewSubtitle={page.view.showPreviewSubtitle}
        showActions={page.view.showHeaderActions}
        onCancelPreview={page.cancelPreview}
        onExecute={page.requestExecute}
      />
      <PageErrorBanner
        message={page.error}
        className="classify-page__error"
        dismissClassName="classify-page__error-dismiss"
        onDismiss={page.clearError}
      />
      <ClassifyPreviewArea
        view={page.view}
        status={page.status}
        progress={page.progress}
        drawer={{ target: page.previewTarget, onClose: page.closePreview }}
        actions={{
          onOpenPreview: page.openPreviewItem,
          onPause: page.pause,
          onResume: page.resume,
          onCancel: page.cancel,
        }}
      />
      {page.view.doneSummary && (
        <ClassifyDonePanel
          summary={page.view.doneSummary}
          cancelled={page.view.cancelled}
          canUndo={page.view.canUndo}
          onUndo={page.undoLastBatch}
          onFinish={page.reset}
        />
      )}
      {page.view.idle && (
        <ClassifyIdleView
          startButton={page.view.startButton}
          historyOpen={page.historyOpen}
          onStart={page.start}
          onOpenHistory={page.openHistory}
          onCloseHistory={page.closeHistory}
        />
      )}
      {page.pendingExecute && (
        <ClassifyModeDialog onChoose={page.chooseMode} onCancel={page.cancelExecute} />
      )}
    </div>
  );
}
