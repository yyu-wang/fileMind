// 分类 store：分类预览、执行进度、暂停/取消控制、撤销。
//
// 不持久化：分类流程为临时操作，每次启动重置
//
// 状态机（ClassifyStatus）：
//   Idle → Previewing → Running ⇄ Paused → Done | Cancelled
//
// 暂停/取消的实质控制逻辑在 T6.5 实现，本 store 仅维护 status 字段

import { create } from 'zustand';
import { fileIpc } from '../lib/ipc';
import type { ExecuteRequest, PreviewRequest, PreviewResponse, UndoResponse } from '../types/ipc';
import { ClassifyStatus } from '../types/models';

interface ProgressState {
  /** 已完成数 */
  done: number;
  /** 总数 */
  total: number;
}

interface ClassifyState {
  /** 当前分类流程状态 */
  status: ClassifyStatus;
  /** 预览结果 */
  preview: PreviewResponse | null;
  /** 执行进度 */
  progress: ProgressState;
  /** 最近批次 ID（用于撤销） */
  lastBatchId: string | null;
  /** 错误信息 */
  error: string | null;

  /** 生成分类预览 */
  generatePreview: (request: PreviewRequest) => Promise<void>;
  /** 执行分类（返回批次 ID 供撤销用） */
  execute: (request: ExecuteRequest) => Promise<void>;
  /** 暂停（T6.5 实现实质逻辑） */
  pause: () => void;
  /** 恢复（T6.5 实现实质逻辑） */
  resume: () => void;
  /** 取消（T6.5 实现实质逻辑） */
  cancel: () => void;
  /** 撤销最近批次 */
  undoLastBatch: () => Promise<void>;
  /** 重置状态 */
  reset: () => void;
  /** 清除错误 */
  clearError: () => void;
}

const INITIAL_PROGRESS: ProgressState = { done: 0, total: 0 };

export const useClassifyStore = create<ClassifyState>()((set, get) => ({
  status: ClassifyStatus.Idle,
  preview: null,
  progress: INITIAL_PROGRESS,
  lastBatchId: null,
  error: null,

  generatePreview: async (request) => {
    set({ status: ClassifyStatus.Previewing, error: null });
    const result = await fileIpc.previewOperations(request);
    if (result.status === 'ok') {
      set({ preview: result.data });
    } else {
      set({ status: ClassifyStatus.Idle, error: result.error });
    }
  },

  execute: async (request) => {
    set({ status: ClassifyStatus.Running, error: null, progress: INITIAL_PROGRESS });
    const result = await fileIpc.executeOperations(request);
    if (result.status === 'ok') {
      const data = result.data;
      set({
        status: ClassifyStatus.Done,
        lastBatchId: data.batch_id,
        progress: {
          done: data.summary.success,
          total: data.summary.total,
        },
      });
    } else {
      set({ status: ClassifyStatus.Idle, error: result.error });
    }
  },

  pause: () => {
    if (get().status === ClassifyStatus.Running) {
      set({ status: ClassifyStatus.Paused });
    }
  },

  resume: () => {
    if (get().status === ClassifyStatus.Paused) {
      set({ status: ClassifyStatus.Running });
    }
  },

  cancel: () => {
    set({ status: ClassifyStatus.Cancelled });
  },

  undoLastBatch: async () => {
    const batchId = get().lastBatchId;
    if (!batchId) {
      set({ error: '无最近批次可撤销' });
      return;
    }
    const result = await fileIpc.undoBatch(batchId);
    if (result.status === 'ok') {
      const data: UndoResponse = result.data;
      set({
        status: ClassifyStatus.Idle,
        lastBatchId: null,
        // UndoResponse 没有 total_count，用 undone_count 作为已完成
        progress: { done: data.undone_count, total: data.undone_count + data.failed_count },
      });
    } else {
      set({ error: result.error });
    }
  },

  reset: () =>
    set({
      status: ClassifyStatus.Idle,
      preview: null,
      progress: INITIAL_PROGRESS,
      lastBatchId: null,
      error: null,
    }),

  clearError: () => set({ error: null }),
}));
