// 分批执行引擎：chunk 循环 + 暂停/取消 + 执行令牌校验 + 打标限并发。
//
// 与 store 分离的原因：循环本身是纯编排（无响应式状态），抽出来后 store 只负责
// 「状态写入 + 生命周期」，异常收尾的部分结果也随返回值一起回到 store，
// 不会被 try/catch 边界吞掉。

import { mapWithConcurrency } from '@/lib/concurrency';
import { fileIpc } from '@/lib/ipc';
import { ipcErrorMessage } from '@/lib/ipcError';
import type { ClassifyPlanItem, ClassifyPreview, PlanItem } from '@/types/ipc';
import type { ClassifyExecMode, ClassifyExecSummary } from './types';

/** 单块执行的文件数上限（分批调用避免单次 IPC 过久）。 */
const CHUNK_SIZE = 50;

/** chunk 内打标（updateFileCategory）并发数：SQLite 单写者下单条 UPDATE 安全，5 路已显著快于串行。 */
const LABEL_CONCURRENCY = 5;

/** 执行循环控制（非响应式：暂停/取消通过它中断 chunk 循环）。 */
export interface ExecControl {
  /** 已请求暂停（循环在每个 chunk 前阻塞等待） */
  paused: boolean;
  /** 已请求取消（循环尽快退出，已完成块保留） */
  cancelled: boolean;
}

/** 执行结果：`abandoned` / `aborted` 为真时调用方需按对应语义收尾。 */
export interface ExecutionOutcome {
  /** 令牌失效（reset 作废在途执行）：调用方不得写任何状态 */
  abandoned: boolean;
  /** 异常中断：调用方按「取消」语义收尾，success/failed 为异常前的部分结果 */
  aborted: boolean;
  success: number;
  failed: number;
  lastBatchId: string | null;
}

/** 汇总执行结果（pending/total 来自预览，success/failed 来自实际执行）。 */
export function buildSummary(
  preview: ClassifyPreview,
  success: number,
  failed: number,
): ClassifyExecSummary {
  return {
    success,
    failed,
    pending: preview.items.filter((item) => item.category_name == null).length,
    total: preview.items.length,
  };
}

/** 分类计划项 → T3.x 执行用 PlanItem（按 mode 选 Move 移动 / Copy 复制）。 */
export function toPlanItem(item: ClassifyPlanItem, mode: ClassifyExecMode): PlanItem {
  return {
    file_id: item.file_id,
    file_name: item.file_name,
    original_path: item.original_path,
    new_path: item.target_path,
    operation: mode === 'copy' ? 'Copy' : 'Move',
    status: item.status,
    conflict_type: item.conflict_type,
  };
}

/** 暂停期间阻塞当前 chunk；返回时是否已被取消。 */
async function waitWhilePaused(control: ExecControl): Promise<boolean> {
  while (control.paused && !control.cancelled) {
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  return control.cancelled;
}

/**
 * 分块执行分类计划：每块调一次 `execute_operations`，成功项再逐项打分类标签。
 *
 * 三条退出路径（与 store 的收尾分支一一对应）：
 * 1. 跑完（含 chunk 级 IPC 错误提前 break）→ 均非 abandoned/aborted
 * 2. 令牌失效（用户已 reset）→ `abandoned`，调用方静默退出
 * 3. 抛出异常 → `aborted`，调用方按「取消」语义收尾并保留部分结果
 *
 * 错误经 `onError` 即时上报（保持原实现的提示时机），不阻断后续 chunk 的打标统计。
 *
 * Args:
 *   plan: 已过滤并按 mode 映射好的执行项
 *   batchId: 预览批次号，随每个 chunk 回传以支撑整批撤销
 *   execItems: 分类计划项（按 file_id 反查待写入的 category_name）
 *   resolveConflicts: 冲突项是否按 Rename 策略一并执行
 *   control: 暂停/取消开关（由 store 的 pause/resume/cancel 写入）
 *   stillHolds: 执行令牌是否仍有效（reset 后为假）
 *   onProgress: 每个 chunk 结束后的进度回调
 *   onError: 错误消息回调（chunk 级 IPC 错误 / 打标失败 / 异常中断）
 */
export async function runBatchedExecution(args: {
  plan: PlanItem[];
  batchId: string;
  execItems: ClassifyPlanItem[];
  resolveConflicts: boolean;
  control: ExecControl;
  stillHolds: () => boolean;
  onProgress: (done: number, lastBatchId: string) => void;
  onError: (message: string) => void;
}): Promise<ExecutionOutcome> {
  const { plan, batchId, execItems, resolveConflicts, control, stillHolds, onProgress, onError } =
    args;
  let success = 0;
  let failed = 0;
  let lastBatchId: string | null = null;
  const abandon = (): ExecutionOutcome => ({
    abandoned: true,
    aborted: false,
    success,
    failed,
    lastBatchId,
  });

  try {
    for (let i = 0; i < plan.length; i += CHUNK_SIZE) {
      if (await waitWhilePaused(control)) break;
      // 每个 await 恢复点先校验令牌：已被 reset 作废则静默退出，不写任何状态。
      if (!stillHolds()) return abandon();
      const chunk = plan.slice(i, i + CHUNK_SIZE);
      const result = await fileIpc.executeOperations({
        batch_id: batchId,
        plan: chunk,
        exclude_file_ids: [],
        resolve_conflicts: resolveConflicts,
      });
      if (!stillHolds()) return abandon();
      if (result.status === 'error') {
        failed += chunk.length;
        onError(result.error);
        break;
      }
      // chunk 内打标限并发（LABEL_CONCURRENCY）：原逐项串行 await 最多 50 次
      // 顺序 IPC 往返；结果统计在并发完成后按同序汇总，语义与串行版一致。
      const outcomes = await mapWithConcurrency(
        result.data.results,
        LABEL_CONCURRENCY,
        async (r) => {
          if (!r.success) return false;
          const execItem = execItems.find((item) => item.file_id === r.file_id);
          // 移动/复制两种模式都打标签到原文件：移动=标记已整理的落库路径，
          // 复制=原文件原地保留但标记已分类（软排除，避免再次被批量选中）
          if (execItem?.category_name) {
            const label = await fileIpc.updateFileCategory(r.file_id, execItem.category_name);
            // FE-C6：打标失败不得静默——文件已移动但 category 未落库会使
            // isOrganized 失效，下轮「全部分类」重复整理；计入失败并提示。
            if (label.status === 'error') {
              onError(`文件已移动但分类标签写入失败：${label.error}`);
              return false;
            }
          }
          return true;
        },
      );
      for (const ok of outcomes) {
        if (ok) {
          success += 1;
        } else {
          failed += 1;
        }
      }
      lastBatchId = result.data.batch_id;
      if (!stillHolds()) return abandon();
      onProgress(success + failed, lastBatchId);
    }
    return { abandoned: false, aborted: false, success, failed, lastBatchId };
  } catch (err) {
    // 先校验令牌：异常可能是「用户已 reset 后旧循环才抛出」，此时不得上报错误
    // （reset 已把 error 清空，晚到的异常消息会把它污染成取消态残留）。
    if (!stillHolds()) return abandon();
    // 异常中断若不把部分结果带回去：store 只能按 0/0 收尾，已完成块与整批撤销
    // 入口会一起丢失（用户看不出实际移动了多少文件）。
    onError(ipcErrorMessage(err));
    return { abandoned: false, aborted: true, success, failed, lastBatchId };
  }
}
