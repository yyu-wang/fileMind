// 智能分类页的派生状态（纯函数）：把「status + preview + 执行汇总」换算成「渲染哪一块、
// 哪些标记位」，页面组件只负责编排与派发回调。
//
// 抽出来的原因：这些判定原先散在 ClassifyPage 的 JSX 里（十余处 `&&` / 三元 / `||`），
// 把该函数推到圈复杂度 38；判定本身是纯逻辑，独立成模块后可直接单测。

import type { ClassifyPreview } from '@/types/ipc';
import { ClassifyStatus } from '@/types/models';
import type { ClassifyExecSummary } from '@/stores/classify/types';

/** 预览中的冲突 / 待确认计数（底部警告条）。 */
export interface ClassifyPreviewCounts {
  /** 目标已存在等需用户决策的项数 */
  conflictCount: number;
  /** 未命中规则与启发式的项数 */
  pendingCount: number;
}

/** 分类页渲染模型：页面按这些标记位决定各区域显隐。 */
export interface ClassifyPageView {
  /** 非空即渲染预览双栏（Done/Cancelled 或无预览时为 null） */
  layoutPreview: ClassifyPreview | null;
  /** 是否展示「正在生成分类预览…」 */
  loading: boolean;
  /** 非空即渲染执行结果面板 */
  doneSummary: ClassifyExecSummary | null;
  /** 结果面板是否按「已取消（部分执行）」呈现 */
  cancelled: boolean;
  /** 结果面板是否展示撤销入口 */
  canUndo: boolean;
  /** 执行中（Running/Paused）：隐藏头部按钮 + 显示进度遮罩 */
  executing: boolean;
  /** 头部按钮组（取消 / 仅执行无冲突项 / 确认执行全部）是否显示 */
  showHeaderActions: boolean;
  /** 头部是否显示「预览分类方案」副标题 */
  showPreviewSubtitle: boolean;
  /** 底部警告条是否显示 */
  showWarning: boolean;
  /** 冲突文件数（警告条文案） */
  conflictCount: number;
  /** 待确认文件数（警告条文案） */
  pendingCount: number;
  /** 是否处于初始态（无预览）：渲染介绍区或历史视图 */
  idle: boolean;
  /** 开始分类按钮：文案 + 禁用态 */
  startButton: StartButtonState;
}

/** 开始分类按钮的渲染状态。 */
export interface StartButtonState {
  /** 按钮文案 */
  label: string;
  /** 是否禁用（尚无文件，或无选中且所有文件均已整理） */
  disabled: boolean;
}

/** 渲染模型的输入：直接取自 store 状态。 */
export interface ClassifyPageViewInput {
  /** 分类流程状态 */
  status: ClassifyStatus;
  /** 当前预览结果 */
  preview: ClassifyPreview | null;
  /** 执行结果汇总 */
  execSummary: ClassifyExecSummary | null;
  /** 是否存在可撤销批次（`lastBatchId != null`） */
  hasUndoBatch: boolean;
  /** 文件页计数：文件总数 / 选中数 / 未整理数 */
  fileCounts: { total: number; selected: number; unorganized: number };
}

/** 开始分类按钮的文案依赖。 */
export interface StartButtonInput {
  /** 文件页是否已扫描出文件 */
  hasFiles: boolean;
  /** 是否存在选中文件 */
  hasSelection: boolean;
  /** 选中文件数（文案展示） */
  selectedCount: number;
  /** 无选中且全部文件均已整理（无可分类目标） */
  noUnorganizedTargets: boolean;
}

/** 统计预览中的冲突项（status=Conflict）与待确认项（stats.pending）。 */
export function countPreviewItems(preview: ClassifyPreview | null): ClassifyPreviewCounts {
  const items = preview?.items ?? [];
  return {
    conflictCount: items.filter((item) => item.status === 'Conflict').length,
    pendingCount: preview?.stats.pending ?? 0,
  };
}

/** 开始分类按钮文案：选中态 > 无可整理目标 > 有文件 > 尚未扫描。 */
export function resolveStartButtonLabel({
  hasFiles,
  hasSelection,
  selectedCount,
  noUnorganizedTargets,
}: StartButtonInput): string {
  if (hasSelection) return `对选中的 ${selectedCount} 个文件开始分类`;
  if (noUnorganizedTargets) return '没有未整理的文件';
  if (hasFiles) return '全部分类';
  return '请先在文件页扫描目录';
}

/** 由 store 状态派生分类页渲染模型（含软排除的「开始分类」按钮状态）。 */
export function resolveClassifyPageView({
  status,
  preview,
  execSummary,
  hasUndoBatch,
  fileCounts,
}: ClassifyPageViewInput): ClassifyPageView {
  const { conflictCount, pendingCount } = countPreviewItems(preview);
  const cancelled = status === ClassifyStatus.Cancelled;
  const finished = cancelled || status === ClassifyStatus.Done;
  const layoutPreview = finished ? null : preview;
  const executing = status === ClassifyStatus.Running || status === ClassifyStatus.Paused;

  // 软排除：无选中时「全部分类」只针对未整理文件；全部已整理则禁用并提示
  const hasFiles = fileCounts.total > 0;
  const hasSelection = fileCounts.selected > 0;
  const noUnorganizedTargets = hasFiles && !hasSelection && fileCounts.unorganized === 0;

  return {
    layoutPreview,
    loading: status === ClassifyStatus.Previewing,
    doneSummary: finished ? execSummary : null,
    cancelled,
    canUndo: hasUndoBatch,
    executing,
    showHeaderActions: layoutPreview !== null && !executing,
    showPreviewSubtitle: layoutPreview !== null,
    showWarning: layoutPreview !== null && (conflictCount > 0 || pendingCount > 0),
    conflictCount,
    pendingCount,
    idle: status === ClassifyStatus.Idle && preview === null,
    startButton: {
      label: resolveStartButtonLabel({
        hasFiles,
        hasSelection,
        selectedCount: fileCounts.selected,
        noUnorganizedTargets,
      }),
      disabled: !hasFiles || noUnorganizedTargets,
    },
  };
}
