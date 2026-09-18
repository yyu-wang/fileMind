// 整批撤销动作（store 的 undoLastBatch）：撤销最近一次执行的移动/打标。
//
// 从 runActions 拆出（该文件逼近 .ts 警告阈值 150）。依赖面按本动作所需最小化
// （只要 set / get），不引用 runActions 的 RunDeps，避免两模块互相依赖。

import { fileIpc } from '@/lib/ipc';
import { ipcErrorMessage } from '@/lib/ipcError';
import { ClassifyStatus } from '@/types/models';
import { useFileStore } from '../fileStore';
import { INITIAL_PROGRESS, type ClassifyState } from './types';

/** 撤销最近整批：无批次时提示；成功回 Idle 并刷新统计。 */
export async function undoLastBatchImpl(deps: {
  set: (patch: Partial<ClassifyState>) => void;
  get: () => ClassifyState;
}): Promise<void> {
  const { set, get } = deps;
  const batchId = get().lastBatchId;
  if (!batchId) {
    set({ error: '无最近批次可撤销' });
    return;
  }
  try {
    const result = await fileIpc.undoBatch(batchId);
    if (result.status === 'ok') {
      set({ status: ClassifyStatus.Idle, lastBatchId: null, progress: INITIAL_PROGRESS });
      // 同 execute：撤销后只刷统计，避免数十万级全量刷新卡 UI
      void useFileStore.getState().loadStats();
    } else {
      set({ error: result.error });
    }
  } catch (err) {
    // 调用方是 `void undoLastBatch()`：抛出会变成 unhandled rejection 且界面无提示
    set({ error: ipcErrorMessage(err) });
  }
}
