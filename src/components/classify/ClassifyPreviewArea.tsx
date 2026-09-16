// 分类预览区域：加载提示 + 预览双栏（含进度遮罩）+ 底部冲突警告条。
//
// 三块的显隐判定统一来自 lib/classifyView.ts 的标记位（view），本组件只按标记位渲染；
// 共用一个 Fragment，DOM 层级与页面内联时一致。

import type { FilePreviewTarget } from '@/components/common/FilePreviewDrawer';
import type { ClassifyPageView } from '@/lib/classifyView';
import type { ProgressState } from '@/stores/classify/types';
import type { ClassifyPlanItem } from '@/types/ipc';
import type { ClassifyStatus } from '@/types/models';

import { ClassifyPreviewSection } from './ClassifyPreviewSection';

interface ClassifyPreviewAreaProps {
  /** 页面渲染模型（loading / layoutPreview / showWarning 与冲突计数） */
  view: ClassifyPageView;
  /** 当前分类流程状态（决定是否显示进度遮罩） */
  status: ClassifyStatus;
  /** 执行进度 */
  progress: ProgressState;
  /** 文件预览抽屉：目标文件 + 关闭回调 */
  drawer: { target: FilePreviewTarget | null; onClose: () => void };
  /** 动作：打开树中文件预览 / 暂停 / 继续 / 取消 */
  actions: {
    onOpenPreview: (item: ClassifyPlanItem) => void;
    onPause: () => void;
    onResume: () => void;
    onCancel: () => void;
  };
}

export function ClassifyPreviewArea({
  view,
  status,
  progress,
  drawer,
  actions,
}: ClassifyPreviewAreaProps) {
  return (
    <>
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
          drawer={drawer}
          actions={actions}
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
    </>
  );
}
