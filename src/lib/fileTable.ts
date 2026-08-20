// 文件列表派生逻辑：状态判定、筛选、排序的纯函数。
//
// 虚拟滚动表需一次性持有全量数组，筛选/排序在客户端完成（见 T6.4 设计）。

import type { FileInfo } from '@/types/ipc';

export type FileStatus = 'categorized' | 'uncategorized';
export type SortKey = 'name' | 'size' | 'time';
export type SortDir = 'asc' | 'desc';

export interface FilterOptions {
  category: string | null;
  status: FileStatus | null;
}

/** 分类态：`category` 非空即已分类（低置信度「待确认」态属 Epic 4）。 */
export function deriveFileStatus(file: FileInfo): FileStatus {
  return file.category == null ? 'uncategorized' : 'categorized';
}

/** 按分类/状态筛选，返回新数组（不改动入参）。 */
export function filterFiles(files: FileInfo[], options: FilterOptions): FileInfo[] {
  return files.filter((file) => {
    if (options.category != null && file.category !== options.category) {
      return false;
    }
    if (options.status != null && deriveFileStatus(file) !== options.status) {
      return false;
    }
    return true;
  });
}

/** 按文件名关键字搜索（大小写不敏感，空串返回全部），返回新数组。 */
export function filterByName(files: FileInfo[], query: string): FileInfo[] {
  const keyword = query.trim().toLowerCase();
  if (!keyword) {
    return files;
  }
  return files.filter((file) => file.file_name.toLowerCase().includes(keyword));
}

/** 分类 tag 配色板（对齐交互原型 tag-blue/green 多色区分）。 */
const CATEGORY_TAG_COLORS = ['blue', 'green', 'amber', 'purple'] as const;

/** 分类名 → 稳定分配的 tag 配色类名（`tag--{color}`），同名分类恒同色。 */
export function categoryTagClass(category: string): string {
  let hash = 0;
  for (let i = 0; i < category.length; i += 1) {
    hash = (hash * 31 + category.charCodeAt(i)) >>> 0;
  }
  const color = CATEGORY_TAG_COLORS[hash % CATEGORY_TAG_COLORS.length];
  return `tag tag--${color}`;
}

const NAME_COLLATOR = new Intl.Collator('zh-Hans-CN', { numeric: true, sensitivity: 'base' });

/** 客户端排序，返回新数组（不改动入参）。时间列用固定格式字符串直接比较。 */
export function sortFiles(files: FileInfo[], key: SortKey, dir: SortDir): FileInfo[] {
  const sorted = [...files];
  const factor = dir === 'asc' ? 1 : -1;
  sorted.sort((a, b) => {
    let cmp = 0;
    switch (key) {
      case 'name':
        cmp = NAME_COLLATOR.compare(a.file_name, b.file_name);
        break;
      case 'size':
        cmp = a.file_size - b.file_size;
        break;
      case 'time':
        cmp = a.updated_at < b.updated_at ? -1 : a.updated_at === b.updated_at ? 0 : 1;
        break;
    }
    return cmp * factor;
  });
  return sorted;
}
