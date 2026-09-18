// 文件列表动作：扫描目录、从库全量拉取、统计、已扫描目录列表。
//
// 与 store 分离的原因：这四个动作共用同一套「IPC 结果分支 + 失败不 rethrow」形态，
// 且 scanFiles / loadAllFiles 共用一个列表请求序号（FE-C4），序号必须与两者同模块。
//
// 依赖通过 ListDeps 注入而非直接引用 store：避免子模块反向 import store 造成循环依赖
// （同 stores/classify/lifecycle.ts）。

import { fileIpc } from '@/lib/ipc';
import { ipcErrorMessage } from '@/lib/ipcError';
import type { FileGet, FileSet, FileState } from './types';

/**
 * 文件列表请求序号（FE-C4）——scanFiles/loadAllFiles 共用。
 * 慢请求（大目录扫描）后发起的快请求先返回时，旧响应到达后序号失配被丢弃，
 * 防止「A 目录晚回覆盖 B 目录」导致 files 与 scanPath 不一致。
 */
let listReqId = 0;

/** 列表动作所需的最小依赖面。 */
export interface ListDeps {
  /** 写入状态（zustand 的 set） */
  set: FileSet;
  /** 读当前状态（zustand 的 get） */
  get: FileGet;
}

/** 扫描完成后刷新统计 + 目录列表（两者都从不 reject，失败只写 error 横幅）。 */
async function refreshScanResult(get: FileGet): Promise<void> {
  await get().loadStats();
  await get().loadScannedDirectories();
}

/** 扫描目录并更新文件列表（成功后顺带刷新统计与已扫描目录）。 */
async function scanFiles(deps: ListDeps, path: string): Promise<void> {
  const { set, get } = deps;
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
      await refreshScanResult(get);
    } else {
      set({ isScanning: false, error: result.error });
    }
  } catch (err) {
    // 不 rethrow：调用方（按钮回调 / 引导流程）要么没包 try，要么把两种失败一视同仁，
    // 错误经由 store.error 横幅呈现
    if (req === listReqId) set({ isScanning: false, error: ipcErrorMessage(err) });
  }
}

/** 从库全量拉取文件列表（刷新用，覆盖 files 并让旧 scanPath 解绑）。 */
async function loadAllFiles(deps: ListDeps): Promise<void> {
  const { set, get } = deps;
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
      await get().loadStats();
    } else {
      set({ isScanning: false, isLoadingList: false, error: result.error });
    }
  } catch (err) {
    if (req === listReqId) {
      set({ isScanning: false, isLoadingList: false, error: ipcErrorMessage(err) });
    }
  }
}

/** 加载文件库统计（stats 与 total 同源）。 */
async function loadStats(deps: ListDeps): Promise<void> {
  const { set } = deps;
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
}

/** 加载已扫描目录列表（目录级移除面板用）。 */
async function loadScannedDirectories(deps: ListDeps): Promise<void> {
  const { set } = deps;
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
}

/**
 * 生成列表动作集合（供 store 展开进 create 的返回对象）。
 *
 * Args:
 *   deps: store 注入的依赖面（见 ListDeps）
 *
 * Returns:
 *   扫描 / 全量拉取 / 统计 / 已扫描目录列表四个动作
 */
export function createListActions(
  deps: ListDeps,
): Pick<FileState, 'scanFiles' | 'loadAllFiles' | 'loadStats' | 'loadScannedDirectories'> {
  return {
    scanFiles: (path) => scanFiles(deps, path),
    loadAllFiles: () => loadAllFiles(deps),
    loadStats: () => loadStats(deps),
    loadScannedDirectories: () => loadScannedDirectories(deps),
  };
}
