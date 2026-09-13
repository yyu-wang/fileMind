// 文件 store：文件列表、扫描状态、文件库统计。
//
// 不持久化：文件列表每次启动重新扫描（数据时效性优先）
//
// IPC 调用：
// - scanDirectory(path) → FileInfo[]
// - getFileStats() → FileStats

import { create } from 'zustand';
import { fileIpc } from '../lib/ipc';
import type { FileInfo, FileStats, ScannedDirectory } from '../types/ipc';
import { useClassifyStore } from './classifyStore';

// FE-C4：文件列表请求序号——scanFiles/loadAllFiles 共用。
// 慢请求（大目录扫描）后发起的快请求先返回时，旧响应到达后序号失配被丢弃，
// 防止「A 目录晚回覆盖 B 目录」导致 files 与 scanPath 不一致。
let listReqId = 0;

/**
 * 把 invoke 层抛出的任意值归一化成可展示的消息。
 *
 * specta 的 typedError 会把命令失败包成 `{status:'error'}`，所以异常路径理论上不可达；
 * 但一旦真的抛出（IPC 通道断开、序列化失败等），若不兜住就会让 `isScanning` 永久为
 * true——按钮全部禁用，用户只能重启应用。
 */
function ipcErrorMessage(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}

interface FileState {
  /** 当前文件列表 */
  files: FileInfo[];
  /** 总文件数（stats 来源） */
  total: number;
  /** 当前扫描路径 */
  scanPath: string | null;
  /** 是否正在扫描 */
  isScanning: boolean;
  /**
   * 是否正在全量拉取文件列表（刷新 / 页面挂载补拉）。
   *
   * 与 `isScanning` 分开：两者都会翻转 `isScanning` 以复用按钮防抖，但空态文案
   * 需要区分「正在扫描目录」与「正在加载列表」——只靠 `isScanning` 无法分辨，
   * 会出现「扫描目录」按钮明明没被点却提示「正在扫描…」。
   */
  isLoadingList: boolean;
  /** 文件库统计 */
  stats: FileStats | null;
  /** 已选中文件的 id 列表 */
  selectedIds: string[];
  /** 已扫描目录列表（目录级移除用） */
  scannedDirectories: ScannedDirectory[];
  /** 错误信息 */
  error: string | null;

  /** 扫描目录并更新文件列表 */
  scanFiles: (path: string) => Promise<void>;
  /** 从库全量拉取文件列表（刷新用，覆盖 files） */
  loadAllFiles: () => Promise<void>;
  /** 加载文件库统计 */
  loadStats: () => Promise<void>;
  /** 加载已扫描目录列表 */
  loadScannedDirectories: () => Promise<void>;
  /** 移除目录：从索引中软删该目录下所有文件 + 清理向量，不删除磁盘文件 */
  removeDirectory: (path: string) => Promise<number>;
  /** 删除文件：把选中的文件移入系统回收站（成功项从列表移除，失败项保留并置 error） */
  deleteFiles: (ids: string[]) => Promise<{ deleted: number; failed: number }>;
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
  isLoadingList: false,
  stats: null,
  selectedIds: [],
  scannedDirectories: [],
  error: null,

  scanFiles: async (path) => {
    const req = ++listReqId;
    set({ isScanning: true, error: null });
    try {
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
        // 扫描完成后刷新统计 + 目录列表
        await useFileStore.getState().loadStats();
        await useFileStore.getState().loadScannedDirectories();
      } else {
        set({ isScanning: false, error: result.error });
      }
    } catch (err) {
      // 不 rethrow：调用方（按钮回调 / 引导流程）要么没包 try，要么把两种失败一视同仁，
      // 错误经由 store.error 横幅呈现
      if (req === listReqId) set({ isScanning: false, error: ipcErrorMessage(err) });
    }
  },

  loadAllFiles: async () => {
    const req = ++listReqId;
    // FE-C4：刷新也是列表变更，进 isScanning 态（FilesPage 刷新按钮防狂点）
    set({ isScanning: true, isLoadingList: true, error: null });
    try {
      const result = await fileIpc.listAllFiles(null);
      if (req !== listReqId) return;
      if (result.status === 'ok') {
        set({
          files: result.data,
          selectedIds: [],
          isScanning: false,
          isLoadingList: false,
          // FE-C4：全量列表覆盖了扫描态列表，旧 scanPath 已不代表 files 来源，
          // 必须清空——否则后续手动分类 joinPath(scanPath,...) 拼错目标根
          scanPath: null,
        });
        await useFileStore.getState().loadStats();
      } else {
        set({ isScanning: false, isLoadingList: false, error: result.error });
      }
    } catch (err) {
      if (req === listReqId) {
        set({ isScanning: false, isLoadingList: false, error: ipcErrorMessage(err) });
      }
    }
  },

  loadStats: async () => {
    try {
      const result = await fileIpc.getFileStats();
      if (result.status === 'ok') {
        set({ stats: result.data, total: result.data.total_files });
      } else {
        set({ error: result.error });
      }
    } catch (err) {
      // 调用方多为 `void loadStats()`，抛出去会变成 unhandled rejection
      set({ error: ipcErrorMessage(err) });
    }
  },

  loadScannedDirectories: async () => {
    try {
      const result = await fileIpc.listScannedDirectories();
      if (result.status === 'ok') {
        set({ scannedDirectories: result.data });
      } else {
        set({ error: result.error });
      }
    } catch (err) {
      set({ error: ipcErrorMessage(err) });
    }
  },

  removeDirectory: async (path) => {
    const result = await fileIpc.removeDirectory(path).catch((err: unknown) => {
      // 保留「失败即 throw」契约（面板据此复位 removing 态），但同样写 error 横幅
      const message = ipcErrorMessage(err);
      set({ error: message });
      throw new Error(message);
    });
    if (result.status !== 'ok') {
      set({ error: result.error });
      throw new Error(result.error);
    }
    // 移除后刷新：文件列表、统计、目录列表
    set((state) => ({
      // 从当前文件列表中移除该目录下的文件（path 以被移除目录开头）
      files: state.files.filter((f) => !f.path.startsWith(`${path}/`)),
      scannedDirectories: state.scannedDirectories.filter((d) => d.path !== path),
      selectedIds: state.selectedIds.filter((id) => {
        const f = state.files.find((x) => x.id === id);
        return f ? !f.path.startsWith(`${path}/`) : true;
      }),
    }));
    await useFileStore.getState().loadStats();
    return result.data.removed_files;
  },

  deleteFiles: async (ids) => {
    if (ids.length === 0) return { deleted: 0, failed: 0 };
    const result = await fileIpc.deleteFiles(ids).catch((err: unknown) => {
      const message = ipcErrorMessage(err);
      set({ error: message });
      throw new Error(message);
    });
    if (result.status !== 'ok') {
      set({ error: result.error });
      throw new Error(result.error);
    }
    const removedIds = new Set<string>();
    let failed = 0;
    for (const item of result.data) {
      if (item.success) {
        removedIds.add(item.file_id);
      } else {
        failed += 1;
      }
    }
    set((state) => ({
      files: state.files.filter((f) => !removedIds.has(f.id)),
      selectedIds: state.selectedIds.filter((id) => !removedIds.has(id)),
      error:
        failed > 0 ? `${failed} 个文件未能移入系统回收站（可能已被外部移动），其余已移入` : null,
    }));
    await useFileStore.getState().loadStats();
    return { deleted: removedIds.size, failed };
  },

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
