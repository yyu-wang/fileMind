// 手动分类（T6.12）的预览重算：把若干文件归入指定分类，重算目标路径、状态与统计。
//
// 单文件（assignCategory）与批量（assignCategories）共用本模块：此前两者各自
// 复制了一份目标路径重算与统计更新逻辑，FE-M1（已分类拒绝覆盖）与 FE-M2
// （手动指定即授权执行、状态重算为 Ok）两处修正在两边各改一遍，容易语义漂移。
// 两个入口**各自的准入判断仍在 store**（单文件要先定位到具体项，批量只看数量），
// 这里只收敛「校验顺序 + 重算」这段共同逻辑。

import type { Category, ClassifyPreview } from '@/types/ipc';
import { isSafeTargetDir, joinPath } from './paths';
import type { ClassifyState } from './types';

/** 手动归类的计算结果；`invalid-target` / `no-output-root` 为拒绝原因。 */
export type ManualAssignOutcome =
  | { kind: 'invalid-target'; categoryName: string }
  | { kind: 'no-output-root' }
  | { kind: 'unchanged' }
  | { kind: 'assigned'; preview: ClassifyPreview; assignedIds: string[] };

/**
 * 计算手动归类后的预览（纯函数，不改动入参对象）。
 *
 * 校验顺序与原实现一致：先 `target_dir` 合法性，再收纳根可用性。
 *
 * Args:
 *   preview: 当前预览
 *   fileIds: 待归类的文件 id（批量入口可能含已分类项，会被跳过）
 *   category: 目标分类
 *   outputRoot: 收纳根（`preview.output_root`，缺失时由 store 兜底扫描根）
 *
 * Returns:
 *   归类结果：命中 0 个可归类项时为 `unchanged`（调用方保持状态不变）
 */
export function computeManualAssign(args: {
  preview: ClassifyPreview;
  fileIds: string[];
  category: Category;
  outputRoot: string | null;
}): ManualAssignOutcome {
  const { preview, fileIds, category, outputRoot } = args;

  // 目标目录合法性校验（相对路径、无穿越），非法拒绝并提示
  const targetDir = category.target_dir.trim();
  if (!isSafeTargetDir(targetDir)) {
    return { kind: 'invalid-target', categoryName: category.name };
  }
  if (!outputRoot) {
    return { kind: 'no-output-root' };
  }

  // 分类目标统一落在收纳根（preview.output_root，扫描根同级的 `<扫描根名>_已分类`），
  // 与 Rust `classify_preview` 的目标拼接同源。
  const idSet = new Set(fileIds);
  const assignedIds: string[] = [];
  const updatedItems = preview.items.map((item) => {
    if (!idSet.has(item.file_id)) return item;
    // 已分类项拒绝覆盖（FE-M1）：重复调用会让 stats.categorized 虚增、pending 重复扣减
    if (item.category_name != null) return item;
    assignedIds.push(item.file_id);
    return {
      ...item,
      category_name: category.name,
      rule_source: 'manual',
      target_path: joinPath(outputRoot, targetDir, item.file_name),
      // FE-M2：手动指定 = 用户显式授权执行——重算 status 置 Ok。旧 Conflict/Error
      // 状态基于旧目标路径或预览时点，已过时；新目标即使同名，Rust 执行端默认
      // Rename 策略兜底，不会覆盖。源文件确已不存在的项，执行时单项失败计入
      // failed 并在摘要呈现，好过静默排除（用户手动指定的分类永远不执行且无提示）。
      status: 'Ok' as const,
      conflict_type: null,
    };
  });

  if (assignedIds.length === 0) return { kind: 'unchanged' };

  return {
    kind: 'assigned',
    assignedIds,
    preview: {
      ...preview,
      items: updatedItems,
      stats: {
        ...preview.stats,
        categorized: preview.stats.categorized + assignedIds.length,
        pending: Math.max(preview.stats.pending - assignedIds.length, 0),
      },
    },
  };
}

/**
 * 把手动归类的计算结果翻译成状态更新（store 直接 `set`）。
 *
 * `unchanged` 返回 null（调用方保持状态不变）；拒绝原因只写 error 横幅，
 * 不动 preview/pendingIds——与「未命中任何可归类项」区分开。
 *
 * Args:
 *   outcome: `computeManualAssign` 的结果
 *   pendingIds: 当前待确认 id 列表（命中项需从中移除）
 *
 * Returns:
 *   状态补丁；无需改动时为 null
 */
export function manualOutcomeToState(
  outcome: ManualAssignOutcome,
  pendingIds: string[],
): Partial<ClassifyState> | null {
  switch (outcome.kind) {
    case 'invalid-target':
      return { error: `分类「${outcome.categoryName}」未配置有效目标目录，无法手动分类` };
    case 'no-output-root':
      return { error: '请先在文件页选择要整理的目录' };
    case 'unchanged':
      return null;
    case 'assigned':
      return {
        preview: outcome.preview,
        pendingIds: pendingIds.filter((id) => !outcome.assignedIds.includes(id)),
        error: null,
      };
  }
}
