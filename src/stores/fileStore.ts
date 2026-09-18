// 文件 store：文件列表、扫描状态、文件库统计。
//
// 不持久化：文件列表每次启动重新扫描（数据时效性优先）
//
// IPC 调用：
// - scanDirectory(path) → FileInfo[]
// - getFileStats() → FileStats
//
// 本文件只保留「初始状态 + 选中态动作 + 动作工厂拼装」（complexity 规则：
// max-lines-per-function 源码上限 60 行），其余按职责落到 ./file/ 子模块；
// 对外导入路径 @/stores/fileStore 与公开符号保持不变。
//   ./file/types.ts     公共类型与依赖面（FileState / FileSet / FileGet）
//   ./file/list.ts      扫描 / 全量拉取 / 统计 / 已扫描目录列表
//   ./file/mutation.ts  目录移除 / 批量删除

import { create } from 'zustand';
import { useClassifyStore } from './classifyStore';
import { createListActions } from './file/list';
import { createMutationActions } from './file/mutation';
import type { FileState } from './file/types';

export const useFileStore = create<FileState>()((set, get) => ({
  files: [],
  total: 0,
  scanPath: null,
  isScanning: false,
  isLoadingList: false,
  stats: null,
  selectedIds: [],
  scannedDirectories: [],
  error: null,

  ...createListActions({ set, get }),
  ...createMutationActions({ set, get }),

  toggleSelect: (id) =>
    set((state) => ({
      selectedIds: state.selectedIds.includes(id)
        ? state.selectedIds.filter((x) => x !== id)
        : [...state.selectedIds, id],
    })),

  setSelection: (ids) => set({ selectedIds: ids }),

  clearSelection: () => {
    // 清除选中意味放弃当前分类意图：同步作废 classifyStore 里上一批的预览缓存
    // （preview / pendingIds / execSummary / 进度），防止返回分类页时仍显示旧结果。
    useClassifyStore.getState().reset();
    set({ selectedIds: [] });
  },

  clearFiles: () => {
    // 清空文件库也意味着之前的分类选择完全作废，同步重置分类页缓存
    useClassifyStore.getState().reset();
    set({ files: [], scanPath: null, selectedIds: [] });
  },

  clearError: () => set({ error: null }),
}));
