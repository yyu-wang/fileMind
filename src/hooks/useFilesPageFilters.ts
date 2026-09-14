// 文件页的搜索/筛选/排序状态（页面级 state，不进 store）。
//
// 抽出为 Hook 的原因（complexity 规则）：页面此前同时持有 7 个 useState，已触
// 「useState > 7 拆分」阈值；而「分类筛选 + 状态筛选 + 文件名搜索 + 排序」这条
// 派生链自身闭环，独立后页面只负责渲染与动作编排。

import { useDeferredValue, useMemo, useState } from 'react';

import {
  filterByName,
  filterFiles,
  sortFiles,
  type FileStatus,
  type SortKey,
  type SortState,
} from '@/lib/fileTable';
import type { FileInfo } from '@/types/ipc';

/** `useFilesPageFilters` 的返回值：筛选状态、可选项与派生出的可见列表。 */
export interface FilesPageFilters {
  /** 分类下拉的可选项（从当前列表去重排序而来） */
  categories: string[];
  categoryFilter: string;
  setCategoryFilter: (value: string) => void;
  statusFilter: '' | FileStatus;
  setStatusFilter: (value: '' | FileStatus) => void;
  searchQuery: string;
  setSearchQuery: (value: string) => void;
  sort: SortState;
  /** 点击表头：同列切换升降序，换列则重置为升序 */
  toggleSort: (key: SortKey) => void;
  /** 过滤 + 搜索 + 排序后的可见列表 */
  visibleFiles: FileInfo[];
}

/**
 * 管理文件页的搜索/筛选/排序，并派生出可见文件列表。
 *
 * Args:
 *   files: 全量文件列表（来自 fileStore）
 *
 * Returns:
 *   筛选状态、分类可选项与 `visibleFiles`
 */
export function useFilesPageFilters(files: FileInfo[]): FilesPageFilters {
  const [categoryFilter, setCategoryFilter] = useState('');
  const [statusFilter, setStatusFilter] = useState<'' | FileStatus>('');
  const [searchQuery, setSearchQuery] = useState('');
  const [sort, setSort] = useState<SortState>({ key: 'name', dir: 'asc' });

  const categories = useMemo(() => {
    const set = new Set<string>();
    for (const file of files) {
      if (file.category) {
        set.add(file.category);
      }
    }
    return Array.from(set).sort();
  }, [files]);

  // 搜索用 deferred 值驱动列表重算：输入框更新（高优先级）不被
  // 全量 O(N log N) 过滤+排序（低优先级）阻塞，万级文件下逐字输入不再卡顿
  const deferredQuery = useDeferredValue(searchQuery);

  const visibleFiles = useMemo(() => {
    const filtered = filterFiles(files, {
      category: categoryFilter || null,
      status: statusFilter || null,
    });
    // 客户端搜索（对齐交互原型搜索框，数据全量在内存）
    return sortFiles(filterByName(filtered, deferredQuery), sort.key, sort.dir);
  }, [files, categoryFilter, statusFilter, deferredQuery, sort]);

  const toggleSort = (key: SortKey) => {
    setSort((prev) => ({
      key,
      dir: prev.key === key ? (prev.dir === 'asc' ? 'desc' : 'asc') : 'asc',
    }));
  };

  return {
    categories,
    categoryFilter,
    setCategoryFilter,
    statusFilter,
    setStatusFilter,
    searchQuery,
    setSearchQuery,
    sort,
    toggleSort,
    visibleFiles,
  };
}
