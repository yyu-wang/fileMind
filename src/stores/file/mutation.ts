// 文件库变更动作：移除已扫描目录（软删索引 + 清向量）与批量删除（移入系统回收站）。
//
// 与 store 分离的原因：两个动作共用同一套失败契约——IPC 失败（status=error 或 invoke 抛出）
// 都要「写 error 横幅 + 抛错」，调用方（ScannedDirectoriesPanel / 工具栏）依赖抛错复位
// 自己的 removing/deleting 态，故把归一逻辑收在 callOrFail 一处。

import { fileIpc } from '@/lib/ipc';
import { ipcErrorMessage } from '@/lib/ipcError';
import type { DeleteFilesResult } from '@/types/ipc';
import type { FileGet, FileSet, FileState } from './types';

/** IPC 返回形态（typedError 包装，见 lib/ipc/fileIpc.ts）。 */
type IpcOutcome<T> = { status: 'ok'; data: T } | { status: 'error'; error: string };

/** 删除结果汇总：成功项 id 集合 + 失败计数。 */
type DeleteOutcome = { removedIds: Set<string>; failed: number };

/** 变更动作所需的最小依赖面。 */
export interface MutationDeps {
  /** 写入状态（zustand 的 set） */
  set: FileSet;
  /** 读当前状态（zustand 的 get） */
  get: FileGet;
}

/** 汇总逐项删除结果（纯函数：不碰 IPC 与 store）。 */
function countDeleteOutcome(items: DeleteFilesResult[]): DeleteOutcome {
  const removedIds = new Set<string>();
  let failed = 0;
  for (const item of items) {
    if (item.success) {
      removedIds.add(item.file_id);
    } else {
      failed += 1;
    }
  }
  return { removedIds, failed };
}

/**
 * 调 IPC 并归一失败：写 error 横幅后抛错。
 *
 * 保留「失败即 throw」契约（面板据此复位 removing 态）；invoke 抛出与 status=error
 * 两种失败都归一成 Error(message)，调用方只需 catch 一种。
 */
async function callOrFail<T>(set: FileSet, invoke: () => Promise<IpcOutcome<T>>): Promise<T> {
  const result = await invoke().catch((err: unknown) => {
    const message = ipcErrorMessage(err);
    set({ error: message });
    throw new Error(message);
  });
  if (result.status !== 'ok') {
    set({ error: result.error });
    throw new Error(result.error);
  }
  return result.data;
}

/** 移除目录：从索引软删该目录下文件，并同步清理文件列表 / 目录列表 / 选中态。 */
async function removeDirectory(deps: MutationDeps, path: string): Promise<number> {
  const { set, get } = deps;
  const data = await callOrFail(set, () => fileIpc.removeDirectory(path));
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
  await get().loadStats();
  return data.removed_files;
}

/** 删除文件：仅移除成功项，失败项保留并在 error 中提示数量。 */
async function deleteFiles(
  deps: MutationDeps,
  ids: string[],
): Promise<{ deleted: number; failed: number }> {
  const { set, get } = deps;
  if (ids.length === 0) return { deleted: 0, failed: 0 };
  const data = await callOrFail(set, () => fileIpc.deleteFiles(ids));
  const { removedIds, failed } = countDeleteOutcome(data);
  set((state) => ({
    files: state.files.filter((f) => !removedIds.has(f.id)),
    selectedIds: state.selectedIds.filter((id) => !removedIds.has(id)),
    error: failed > 0 ? `${failed} 个文件未能移入系统回收站（可能已被外部移动），其余已移入` : null,
  }));
  await get().loadStats();
  return { deleted: removedIds.size, failed };
}

/**
 * 生成变更动作集合（供 store 展开进 create 的返回对象）。
 *
 * Args:
 *   deps: store 注入的依赖面（见 MutationDeps）
 *
 * Returns:
 *   移除目录与批量删除两个动作
 */
export function createMutationActions(
  deps: MutationDeps,
): Pick<FileState, 'removeDirectory' | 'deleteFiles'> {
  return {
    removeDirectory: (path) => removeDirectory(deps, path),
    deleteFiles: (ids) => deleteFiles(deps, ids),
  };
}
