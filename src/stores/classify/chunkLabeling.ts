// chunk 执行结果的分类打标（从 executor 拆出：该文件逼近 .ts 警告阈值 150）。
//
// 限并发 + 失败上报：移动/复制两种模式都把标签写到原文件路径；打标失败不得静默
// （FE-C6），否则 isOrganized 失效、下轮「全部分类」重复整理。

import { mapWithConcurrency } from '@/lib/concurrency';
import { fileIpc } from '@/lib/ipc';
import type { ClassifyPlanItem, ExecuteResult } from '@/types/ipc';

/** chunk 内打标（updateFileCategory）并发数：SQLite 单写者下单条 UPDATE 安全，5 路已显著快于串行。 */
const LABEL_CONCURRENCY = 5;

/**
 * 对一批执行结果打分类标签，返回逐项是否成功（顺序与入参一致）。
 *
 * chunk 内打标限并发（:data:`LABEL_CONCURRENCY`）：原逐项串行 await 最多 50 次
 * 顺序 IPC 往返；结果统计在并发完成后按同序汇总，语义与串行版一致。
 *
 * Args:
 *   results: 本 chunk 的执行结果（`execute_operations` 返回）
 *   execItems: 分类计划项（按 file_id 反查待写入的 category_name）
 *   onError: 错误消息回调（打标失败即上报，不阻断同 chunk 其它项）
 */
export async function labelChunkResults(
  results: ExecuteResult[],
  execItems: ClassifyPlanItem[],
  onError: (message: string) => void,
): Promise<boolean[]> {
  return mapWithConcurrency(results, LABEL_CONCURRENCY, async (r) => {
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
  });
}
