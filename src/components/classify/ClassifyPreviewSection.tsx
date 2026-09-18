// 智能分类预览主体：左树 + 右统计 + 文件预览抽屉 + 执行进度遮罩。
//
// 按钮不在这里（收敛到页面头部）；执行中由本组件渲染进度遮罩覆盖预览。

import { LazyFilePreviewDrawer } from '@/components/common/LazyFilePreviewDrawer';
import type { FilePreviewTarget } from '@/components/common/FilePreviewDrawer';
import type { ProgressState } from '@/stores/classify/types';
import type { ClassifyPlanItem, ClassifyPreview } from '@/types/ipc';
import { ClassifyStatus } from '@/types/models';

import { ClassifyPreviewTree } from './ClassifyPreviewTree';
import { ClassifyProgressOverlay } from './ClassifyProgressOverlay';
import { ClassifyStatsPanel } from './ClassifyStatsPanel';

interface ClassifyPreviewSectionProps {
  /** 分类预览结果（含 stats 与逐项数据） */
  preview: ClassifyPreview;
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

export function ClassifyPreviewSection({
  preview,
  status,
  progress,
  drawer,
  actions,
}: ClassifyPreviewSectionProps) {
  const executing = status === ClassifyStatus.Running || status === ClassifyStatus.Paused;

  return (
    <div className="classify-layout">
      {/* 左：树形分类预览（按钮在头部，此处仅展示） */}
      <div className="classify-tree">
        <ClassifyPreviewTree preview={preview} onOpenPreview={actions.onOpenPreview} />
      </div>
      {/* 右：统计面板 */}
      <ClassifyStatsPanel preview={preview} />
      {/* 右：文件预览抽屉（点击树中文件名打开；key 保证切换文件时重挂载重置状态） */}
      <LazyFilePreviewDrawer
        key={drawer.target?.path ?? 'none'}
        file={drawer.target}
        onClose={drawer.onClose}
      />
      {executing && (
        <ClassifyProgressOverlay
          progress={progress}
          status={status}
          onPause={actions.onPause}
          onResume={actions.onResume}
          onCancel={actions.onCancel}
        />
      )}
    </div>
  );
}
