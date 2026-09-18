// 分类预览树的分组与展示辅助（纯函数）：分组规则、组名常量、目标子目录文案。
//
// 从 ClassifyPreviewTree 抽出的原因：分组与组名判定原先是该组件渲染回调的一部分，
// 组件按视觉区域拆出子组件（分组面板 / 文件行）后这些纯函数被多处引用——留在组件
// 文件里会让子组件反向 import 组件，形成循环依赖。

import { PENDING_NAME } from '@/stores/classifyStore';
import type { ClassifyPlanItem } from '@/types/ipc';

/** 分类预览树的一个分组：分类组 / 冲突组 / 待确认组。 */
export interface PreviewGroup {
  /** 组展示名（分类名 / 「⚠️ 冲突」/「❓ 待确认」） */
  name: string;
  /** 组内文件项 */
  items: ClassifyPlanItem[];
}

/** 冲突组展示名。 */
export const CONFLICT_NAME = '⚠️ 冲突';
/** 待确认组展示名（沿用 store 常量，保持统一）。 */
export const PENDING_LABEL = `❓ ${PENDING_NAME}`;

/**
 * 按原型分组：分类组（排序）→ 冲突组 → 待确认组（恒排最后）。
 * 冲突项独立成组，不再混入对应分类组（与原型「⚠️ 冲突 (N)」一致）。
 */
export function groupPreviewItems(items: ClassifyPlanItem[]): PreviewGroup[] {
  const conflict: ClassifyPlanItem[] = [];
  const pending: ClassifyPlanItem[] = [];
  const byCategory = new Map<string, ClassifyPlanItem[]>();

  for (const item of items) {
    if (item.status === 'Conflict') {
      conflict.push(item);
      continue;
    }
    if (item.category_name == null) {
      pending.push(item);
      continue;
    }
    const list = byCategory.get(item.category_name) ?? [];
    list.push(item);
    byCategory.set(item.category_name, list);
  }

  const groups: PreviewGroup[] = [...byCategory.entries()]
    .sort(([a], [b]) => a.localeCompare(b, 'zh-Hans-CN'))
    .map(([name, list]) => ({ name, items: list }));
  if (conflict.length > 0) {
    groups.push({ name: CONFLICT_NAME, items: conflict });
  }
  if (pending.length > 0) {
    groups.push({ name: PENDING_LABEL, items: pending });
  }
  return groups;
}

/** 从 target_path 提取相对收纳根的目标子目录（`/财务/`），供树子节点展示。 */
export function targetSubdir(targetPath: string, root: string | null): string {
  if (!root) return '';
  // FE-m2：加路径边界检查——startsWith 无边界时 /a/dir 误匹配 /a/dir2
  if (targetPath !== root && !targetPath.startsWith(root + '/')) return '';
  const rest = targetPath === root ? '' : targetPath.slice(root.length + 1);
  const dir = rest.includes('/') ? rest.slice(0, rest.lastIndexOf('/')) : '';
  return dir ? ` → /${dir}/` : '';
}
