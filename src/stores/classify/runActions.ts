// 执行动作：分批执行的编排（重入守卫 / 计划筛选 / 令牌取号 / 收尾写状态）。
//
// 与 store 分离的原因：`execute` 的编排是一整块流程，留在 create 回调里会把回调推过
// 函数行数阈值。实现是模块级具名函数、工厂只做转发——工厂自身也在函数行数阈值内。
//
// 同目录拆分（原单文件 181 行，逼近 .ts 警告阈值 150）：
//   - ./executionToken.ts  执行令牌（FE-C5）：取号 / 作废 / 持有判定
//   - ./undoActions.ts     整批撤销（store 的 undoLastBatch）
// 本文件保留执行链路本体与动作工厂；`createRunActions` / `invalidateExecution`
// 仍是 classifyStore 的既有导入路径。

import type { ClassifyPlanItem, ClassifyPreview } from '@/types/ipc';
import { ClassifyStatus } from '@/types/models';
import { useFileStore } from '../fileStore';
import { runBatchedExecution, type ExecControl, type ExecutionOutcome } from './executor';
import { buildSummary, toPlanItem } from './executionPlan';
import { beginExecution, holdsToken } from './executionToken';
import type { ClassifyExecMode, ClassifyState } from './types';
import { undoLastBatchImpl } from './undoActions';

/** 执行/撤销动作所需的最小依赖面。 */
export interface RunDeps {
  /** 写入状态（zustand 的 set） */
  set: (patch: Partial<ClassifyState>) => void;
  /** 读当前状态（zustand 的 get） */
  get: () => ClassifyState;
  /** 执行循环的非响应式开关（暂停/取消由它中断 chunk 循环） */
  control: ExecControl;
}

/** 执行中/暂停中拒绝再次进入（FE-C1：防止双执行循环交错、operations_log 双写污染撤销链）。 */
function isExecuteBlocked(status: ClassifyStatus): boolean {
  return status === ClassifyStatus.Running || status === ClassifyStatus.Paused;
}

/** 本次可执行项：已分类，且（确认执行全部 或 该项无冲突）。 */
function pickExecItems(preview: ClassifyPreview, resolveConflicts: boolean): ClassifyPlanItem[] {
  return preview.items.filter(
    (item) => item.category_name != null && (resolveConflicts || item.status === 'Ok'),
  );
}

/** 无可执行项时的收尾补丁：保留 lastBatchId（上次批次仍可撤销），并给出原因提示。 */
function noExecutablePatch(preview: ClassifyPreview): Partial<ClassifyState> {
  const hasCategorized = preview.items.some((i) => i.category_name != null);
  return {
    status: ClassifyStatus.Done,
    execSummary: buildSummary(preview, 0, 0),
    error: hasCategorized ? '没有可执行的分类项（文件已分类或目标冲突）' : null,
  };
}

/**
 * 执行收尾写状态。
 *
 * 令牌失效（用户已 reset）时不写任何状态；异常中断按「取消」语义收尾——
 * 保留已完成块与整批撤销入口，execSummary 呈现真实的部分结果（而非 0/0）。
 */
function settleExecution(deps: RunDeps, preview: ClassifyPreview, outcome: ExecutionOutcome): void {
  const { set, control } = deps;
  if (outcome.abandoned) return;
  if (outcome.aborted) control.cancelled = true;
  set({
    status: control.cancelled ? ClassifyStatus.Cancelled : ClassifyStatus.Done,
    execSummary: buildSummary(preview, outcome.success, outcome.failed),
  });
}

/**
 * 分块执行已确认分类。
 *
 * Args:
 *   deps: store 注入的依赖面
 *   resolveConflicts: 冲突项是否按 Rename 策略一并执行（确认执行全部）
 *   mode: 移动还是复制
 */
async function executeImpl(
  deps: RunDeps,
  resolveConflicts: boolean,
  mode: ClassifyExecMode,
): Promise<void> {
  const { set, get, control } = deps;
  if (isExecuteBlocked(get().status)) return;
  const preview = get().preview;
  if (!preview) {
    set({ status: ClassifyStatus.Idle, error: '尚未生成分类预览' });
    return;
  }
  // resolveConflicts=true（确认执行全部）：冲突项也纳入 plan，由 Rust 按 Rename 重算执行
  const execItems = pickExecItems(preview, resolveConflicts);
  if (execItems.length === 0) {
    // 本次无可执行项：全部待确认（未分类）或已分类但目标冲突。
    set(noExecutablePatch(preview));
    return;
  }

  const plan = execItems.map((item) => toPlanItem(item, mode));
  const token = beginExecution(set, control, plan.length);
  /** 当前执行是否仍持有令牌（reset 会作废在途执行的写状态权利）。 */
  const stillHolds = (): boolean => holdsToken(token);

  // 执行循环与异常收尾都在 runBatchedExecution 内：它把「已完成的部分结果」
  // 一并带回，store 不会因为异常边界拿不到 success/failed（那样只能按 0/0 收尾）。
  const outcome = await runBatchedExecution({
    plan,
    batchId: preview.batch_id,
    execItems,
    resolveConflicts,
    control,
    stillHolds,
    onProgress: (done, lastBatchId) => set({ progress: { done, total: plan.length }, lastBatchId }),
    onError: (message) => set({ error: message }),
  });
  settleExecution(deps, preview, outcome);
  // 移动/打标已落库：只刷新轻量统计，不拉全量文件列表——
  // 文件库可达数十万级，全量刷新（loadAllFiles）会卡死 UI；列表由用户手动「刷新」。
  // 异常中断同样可能已移动部分文件，故与正常路径一致地刷新。
  void useFileStore.getState().loadStats();
}

/**
 * 生成执行/撤销动作（工厂只转发，实现见同目录模块级函数）。
 *
 * Args:
 *   deps: store 注入的依赖面
 *
 * Returns:
 *   分块执行与整批撤销动作
 */
export function createRunActions(deps: RunDeps): Pick<ClassifyState, 'execute' | 'undoLastBatch'> {
  return {
    execute: (resolveConflicts = false, mode = 'move') => executeImpl(deps, resolveConflicts, mode),
    undoLastBatch: () => undoLastBatchImpl(deps),
  };
}

export { invalidateExecution } from './executionToken';
