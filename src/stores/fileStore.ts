// 文件 store：文件列表、扫描状态、文件库统计。
//
// 不持久化：文件列表每次启动重新扫描（数据时效性优先）
//
// IPC 调用：
// - scanDirectory(path) → FileInfo[]
// - getFileStats() → FileStats

import { create } from 'zustand';
import { fileIpc } from '../lib/ipc';
import type { FileInfo, FileStats } from '../types/ipc';

interface FileState {
  /** 当前文件列表 */
  files: FileInfo[];
  /** 总文件数（stats 来源） */
  total: number;
  /** 当前扫描路径 */
  scanPath: string | null;
  /** 是否正在扫描 */
  isScanning: boolean;
  /** 文件库统计 */
  stats: FileStats | null;
  /** 错误信息 */
  error: string | null;

  /** 扫描目录并更新文件列表 */
  scanFiles: (path: string) => Promise<void>;
  /** 加载文件库统计 */
  loadStats: () => Promise<void>;
  /** 清空文件列表 */
  clearFiles: () => void;
  /** 清除错误 */
  clearError: () => void;
}

export const useFileStore = create<FileState>()((set) => ({
  files: [],
  total: 0,
  scanPath: null,
  isScanning: false,
  stats: null,
  error: null,

  scanFiles: async (path) => {
    set({ isScanning: true, error: null });
    const result = await fileIpc.scanDirectory(path);
    if (result.status === 'ok') {
      set({
        files: result.data,
        scanPath: path,
        isScanning: false,
      });
      // 扫描完成后刷新统计
      await useFileStore.getState().loadStats();
    } else {
      set({ isScanning: false, error: result.error });
    }
  },

  loadStats: async () => {
    const result = await fileIpc.getFileStats();
    if (result.status === 'ok') {
      set({ stats: result.data, total: result.data.total_files });
    } else {
      set({ error: result.error });
    }
  },

  clearFiles: () => set({ files: [], scanPath: null }),

  clearError: () => set({ error: null }),
}));
