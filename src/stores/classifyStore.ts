// 分类 store：分类预览、分块执行、暂停/取消、整批撤销。
//
// 状态机（ClassifyStatus）：
//   Idle → Previewing → Running ⇄ Paused → Done | Cancelled
// 预览态用 `preview != null` 区分（预览成功后 status 回到 Idle，页面渲染预览树）。
//
// 执行复用 T3.x `execute_operations`（每项带各自 `new_path` 的 PlanItem + Skip 冲突策略），
// 共享链式撤销；成功项再逐项调 `update_file_category` 打分类标签。
//
// 本文件只保留「状态持有 + 动作编排」（complexity 规则：.ts 强制上限 250 行），
// 动作实现按职责落到 ./classify/ 子模块；对外导入路径 @/stores/classifyStore 与
// 公开符号保持不变。
//   ./classify/types.ts           公共类型与常量
//   ./classify/paths.ts           目标路径拼接与校验
//   ./classify/executor.ts        分批执行引擎（含结果汇总）
//   ./classify/manualAssign.ts    手动分类的预览重算与状态映射
//   ./classify/manualActions.ts   手动分类动作（单文件 / 批量）
//   ./classify/previewActions.ts  生成预览（重入守卫 + 过期响应丢弃）
//   ./classify/categoryActions.ts 分类缓存加载与强制重拉
//   ./classify/runActions.ts      分批执行与整批撤销
//   ./classify/lifecycle.ts       暂停/继续/取消/复位/清错

import { create } from 'zustand';
import { ClassifyStatus } from '../types/models';
import { createCategoryActions } from './classify/categoryActions';
import type { ExecControl } from './classify/executor';
import { createLifecycleActions } from './classify/lifecycle';
import { createManualAssignActions } from './classify/manualActions';
import { createPreviewActions, invalidatePreview } from './classify/previewActions';
import { createRunActions, invalidateExecution } from './classify/runActions';
import { INITIAL_PROGRESS, type ClassifyState } from './classify/types';

export { PENDING_NAME } from './classify/types';
export type { ClassifyExecMode, ClassifyExecSummary } from './classify/types';

/** 执行循环控制（非响应式：暂停/取消通过它中断 chunk 循环）。 */
const control: ExecControl = { paused: false, cancelled: false };

export const useClassifyStore = create<ClassifyState>()((set, get) => ({
  status: ClassifyStatus.Idle,
  preview: null,
  pendingIds: [],
  categories: [],
  progress: INITIAL_PROGRESS,
  execSummary: null,
  lastBatchId: null,
  error: null,

  ...createLifecycleActions({
    set,
    status: () => get().status,
    control,
    invalidateInFlight: () => {
      invalidateExecution();
      invalidatePreview();
    },
  }),

  ...createPreviewActions({ set, get }),
  ...createCategoryActions({ set, get }),
  ...createManualAssignActions({ set, get }),
  ...createRunActions({ set, get, control }),
}));
