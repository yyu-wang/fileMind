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

// FE-C4：文件列表请求序号——scanFiles/loadAllFiles 共用。
// 慢请求（大目录扫描）后发起的快请求先返回时，旧响应到达后序号失配被丢弃，
// 防止「A 目录晚回覆盖 B 目录」导致 files 与 scanPath 不一致。
let listReqId = 0;

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
  /** 已选中文件的 id 列表 */
  selectedIds: string[];
  /** 错误信息 */
  error: string | null;

  /** 扫描目录并更新文件列表 */
  scanFiles: (path: string) => Promise<void>;
  /** 从库全量拉取文件列表（刷新用，覆盖 files） */
  loadAllFiles: () => Promise<void>;
  /** 加载文件库统计 */
  loadStats: () => Promise<void>;
  /** 切换单个文件的选中态 */
  toggleSelect: (id: string) => void;
  /** 批量设置选中（全选/清空用） */
  setSelection: (ids: string[]) => void;
  /** 清空选中 */
  clearSelection: () => void;
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
  selectedIds: [],
  error: null,

  scanFiles: async (path) => {
    const req = ++listReqId;
    set({ isScanning: true, error: null });
    const result = await fileIpc.scanDirectory(path);
    // FE-C4：期间有更新的列表请求发起，本次响应已过期，丢弃
    if (req !== listReqId) return;
    if (result.status === 'ok') {
      set({
        files: result.data,
        scanPath: path,
        isScanning: false,
        selectedIds: [],
      });
      // 扫描完成后刷新统计
      await useFileStore.getState().loadStats();
    } else {
      set({ isScanning: false, error: result.error });
    }
  },

  loadAllFiles: async () => {
    const req = ++listReqId;
    // FE-C4：刷新也是列表变更，进 isScanning 态（FilesPage 刷新按钮防狂点）
    set({ isScanning: true, error: null });
    const result = await fileIpc.listAllFiles(null);
    if (req !== listReqId) return;
    if (result.status === 'ok') {
      set({
        files: result.data,
        selectedIds: [],
        isScanning: false,
        // FE-C4：全量列表覆盖了扫描态列表，旧 scanPath 已不代表 files 来源，
        // 必须清空——否则后续手动分类 joinPath(scanPath,...) 拼错目标根
        scanPath: null,
      });
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

  toggleSelect: (id) =>
    set((state) => ({
      selectedIds: state.selectedIds.includes(id)
        ? state.selectedIds.filter((x) => x !== id)
        : [...state.selectedIds, id],
    })),

  setSelection: (ids) => set({ selectedIds: ids }),

  clearSelection: () => set({ selectedIds: [] }),

  clearFiles: () => set({ files: [], scanPath: null, selectedIds: [] }),

  clearError: () => set({ error: null }),
}));
