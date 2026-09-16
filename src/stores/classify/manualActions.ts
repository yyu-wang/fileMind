// 手动分类动作（T6.12）：单文件与批量指定分类。
//
// 与 store 分离的原因：两个入口的准入判断（单文件要定位到具体项、批量只看数量）
// 与「重算 → 翻译成状态补丁」的调用链是自成一体的流程，留在 create 回调里会把回调
// 推过函数行数阈值。预览重算与状态补丁本身仍在 ./manualAssign（纯函数）。

import type { Category } from '@/types/ipc';
import { useFileStore } from '../fileStore';
import { computeManualAssign, manualOutcomeToState } from './manualAssign';
import type { ClassifyState } from './types';

/** 手动分类动作所需的最小依赖面。 */
export interface ManualDeps {
  /** 写入状态（zustand 的 set） */
  set: (patch: Partial<ClassifyState>) => void;
  /** 读当前状态（zustand 的 get） */
  get: () => ClassifyState;
}

/**
 * 生成手动分类动作（供 store 展开进 create 的返回对象）。
 *
 * Args:
 *   deps: store 注入的依赖面
 *
 * Returns:
 *   单文件与批量指定分类动作
 */
export function createManualAssignActions(
  deps: ManualDeps,
): Pick<ClassifyState, 'assignCategory' | 'assignCategories'> {
  const { set, get } = deps;

  /** 重算预览并写状态（两个入口共用的「计算 + set 补丁」链路）。 */
  const applyAssign = (fileIds: string[], category: Category): void => {
    const { preview, pendingIds } = get();
    if (!preview) return;
    const outcome = computeManualAssign({
      preview,
      fileIds,
      category,
      outputRoot: preview.output_root || useFileStore.getState().scanPath || null,
    });
    const patch = manualOutcomeToState(outcome, pendingIds);
    if (patch) set(patch);
  };

  return {
    assignCategory: (fileId, category) => {
      const preview = get().preview;
      if (!preview) return;
      const item = preview.items.find((i) => i.file_id === fileId);
      // FE-M1：与批量版 assignCategories 对齐——已分类项拒绝覆盖，
      // 避免重复调用导致分类被覆盖、stats.categorized 虚增、pending 重复扣减
      if (!item || item.category_name != null) return;
      applyAssign([fileId], category);
    },

    assignCategories: (fileIds, category) => {
      if (fileIds.length === 0) return;
      applyAssign(fileIds, category);
    },
  };
}
