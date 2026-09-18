// 预览 → 执行计划/汇总的纯映射（从 executor 拆出：该文件逼近 .ts 警告阈值 150）。
//
// 无副作用、不碰 IPC 与状态，故与「执行引擎」分开放，供执行引擎与动作层共用。

import type { ClassifyPlanItem, ClassifyPreview, PlanItem } from '@/types/ipc';
import type { ClassifyExecMode, ClassifyExecSummary } from './types';

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
