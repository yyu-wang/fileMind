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
