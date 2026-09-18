// 智能分类页的控制器：订阅 store、派生渲染模型，并把三条子链路的 handle 汇总给页面。
//
// 页面函数只留编排（把 handle 分发给头部与各区域组件），页面级逻辑按职责拆到三个 Hook：
//   useClassifyPreviewFlow   预览的自动发起 / 开始 / 取消
//   useClassifyExecuteFlow   「分类方式」弹窗与待执行参数
//   useClassifyPreviewDrawer 树中文件的预览抽屉
// 各区域显隐判定仍统一来自 lib/classifyView.ts（页面不做判断）。

import { useState } from 'react';

import { useClassifyExecuteFlow, type ClassifyExecuteFlow } from '@/hooks/useClassifyExecuteFlow';
import {
  useClassifyPreviewDrawer,
  type ClassifyPreviewDrawer,
} from '@/hooks/useClassifyPreviewDrawer';
import { useClassifyPreviewFlow, type ClassifyPreviewFlow } from '@/hooks/useClassifyPreviewFlow';
import { resolveClassifyPageView, type ClassifyPageView } from '@/lib/classifyView';
import { isOrganized } from '@/lib/fileTable';
import type { ProgressState } from '@/stores/classify/types';
import { useClassifyStore } from '@/stores/classifyStore';
import { useFileStore } from '@/stores/fileStore';
import type { ClassifyStatus } from '@/types/models';

/** `useClassifyPageController` 的对外出口（页面据此编排各区域）。 */
export interface ClassifyPageController
  extends ClassifyPreviewFlow, ClassifyExecuteFlow, ClassifyPreviewDrawer {
  /** 渲染模型：各区域显隐与「开始分类」按钮状态 */
  view: ClassifyPageView;
  /** 当前扫描根路径（头部副标题） */
  scanPath: string | null;
  /** 分类流程状态（预览区的进度遮罩用） */
  status: ClassifyStatus;
  /** 执行进度 */
  progress: ProgressState;
  /** 页面级错误提示 */
  error: string | null;
  /** 关闭错误横幅 */
  clearError: () => void;
  /** 暂停执行 */
  pause: () => void;
  /** 继续执行 */
  resume: () => void;
  /** 取消执行（已执行块保留，可整批撤销） */
  cancel: () => void;
  /** 撤销最近整批 */
  undoLastBatch: () => void;
  /** 回到分类页初始态 */
  reset: () => void;
  /** 是否展示分类历史视图 */
  historyOpen: boolean;
  /** 进入分类历史视图 */
  openHistory: () => void;
  /** 退出分类历史视图 */
  closeHistory: () => void;
}

/** 订阅分类页所需数据并汇总区域动作。 */
export function useClassifyPageController(): ClassifyPageController {
  const files = useFileStore((s) => s.files);
  const scanPath = useFileStore((s) => s.scanPath);
  const selectedIds = useFileStore((s) => s.selectedIds);

  const status = useClassifyStore((s) => s.status);
  const preview = useClassifyStore((s) => s.preview);
  const progress = useClassifyStore((s) => s.progress);
  const execSummary = useClassifyStore((s) => s.execSummary);
  const lastBatchId = useClassifyStore((s) => s.lastBatchId);
  const error = useClassifyStore((s) => s.error);
  const clearError = useClassifyStore((s) => s.clearError);
  const pause = useClassifyStore((s) => s.pause);
  const resume = useClassifyStore((s) => s.resume);
  const cancel = useClassifyStore((s) => s.cancel);
  const undoLastBatch = useClassifyStore((s) => s.undoLastBatch);
  const reset = useClassifyStore((s) => s.reset);

  const flow = useClassifyPreviewFlow();
  const execution = useClassifyExecuteFlow();
  const drawer = useClassifyPreviewDrawer();
  const [historyOpen, setHistoryOpen] = useState(false);

  const view = resolveClassifyPageView({
    status,
    preview,
    execSummary,
    hasUndoBatch: lastBatchId !== null,
    fileCounts: {
      total: files.length,
      selected: selectedIds.length,
      unorganized: files.filter((f) => !isOrganized(f)).length,
    },
  });

  return {
    view,
    scanPath,
    status,
    progress,
    error,
    clearError,
    pause,
    resume,
    cancel,
    undoLastBatch,
    reset,
    historyOpen,
    openHistory: () => setHistoryOpen(true),
    closeHistory: () => setHistoryOpen(false),
    ...flow,
    ...execution,
    ...drawer,
  };
}
