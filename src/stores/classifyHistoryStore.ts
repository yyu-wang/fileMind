// 分类历史 store：批次列表 / 批次明细 / 整批撤销（供智能分类页「分类历史」视图）。
//
// 数据源为 T3.4 的 operations_log 审计日志（链式哈希、仅插入）：
//   loadHistory → get_operation_history（批次摘要，按时间倒序）
//   openBatch   → get_batch_detail（单批次逐行日志）
//   undoBatch   → undo_batch（反向操作链撤销，成功后刷新列表）
//
// 是否可撤销由后端判定（can_undo：done && !has_delete && 24h 窗口内），
// 前端只做展示；二次确认对话框在页面层处理，store 不感知。

import { create } from 'zustand';
import { fileIpc } from '../lib/ipc';
import type { BatchDetailResponse, OperationBatchSummary } from '../types/ipc';
import { useFileStore } from './fileStore';

/** 单页批次条数（后端分页从 1 起，暂取最近一页）。 */
const HISTORY_PAGE_SIZE = 20;

interface ClassifyHistoryState {
  /** 批次摘要列表（按时间倒序） */
  batches: OperationBatchSummary[];
  /** 当前打开的批次明细（null=列表视图） */
  detail: BatchDetailResponse | null;
  /** 是否加载中 */
  loading: boolean;
  /** 是否正在撤销 */
  undoing: boolean;
  /** 错误信息 */
  error: string | null;

  /** 加载历史批次列表 */
  loadHistory: () => Promise<void>;
  /** 打开单个批次明细 */
  openBatch: (batchId: string) => Promise<void>;
  /** 关闭明细，返回列表 */
  closeBatch: () => void;
  /** 撤销指定批次（成功后刷新列表；二次确认在 UI 层完成） */
  undoBatch: (batchId: string) => Promise<void>;
  /** 重置（退出历史视图时清理） */
  reset: () => void;
  /** 清除错误 */
  clearError: () => void;
}

// FE-m8：请求序号守卫——快速连续调用时丢弃过期请求的结果
let historyReqSeq = 0;
let batchReqSeq = 0;

export const useClassifyHistoryStore = create<ClassifyHistoryState>()((set, get) => ({
  batches: [],
  detail: null,
  loading: false,
  undoing: false,
  error: null,

  loadHistory: async () => {
    const seq = ++historyReqSeq;
    set({ error: null, loading: true });
    const result = await fileIpc.getOperationHistory(1, HISTORY_PAGE_SIZE);
    if (seq !== historyReqSeq) return; // 已被新请求取代
    if (result.status === 'ok') {
      set({ batches: result.data.batches, loading: false });
    } else {
      set({ error: result.error, loading: false });
    }
  },

  openBatch: async (batchId) => {
    const seq = ++batchReqSeq;
    set({ error: null, loading: true });
    const result = await fileIpc.getBatchDetail(batchId);
    if (seq !== batchReqSeq) return; // 已被新请求取代
    if (result.status === 'ok') {
      set({ detail: result.data, loading: false });
    } else {
      set({ error: result.error, loading: false });
    }
  },

  closeBatch: () => set({ detail: null }),

  undoBatch: async (batchId) => {
    set({ error: null, undoing: true });
    const result = await fileIpc.undoBatch(batchId);
    if (result.status === 'ok') {
      // 撤销后刷新列表（该批次状态变为 undone）；文件库只刷统计，避免全量刷新卡 UI
      await get().loadHistory();
      void useFileStore.getState().loadStats();
      set({ undoing: false });
    } else {
      set({ error: result.error, undoing: false });
    }
  },

  reset: () => set({ batches: [], detail: null, loading: false, undoing: false, error: null }),
  clearError: () => set({ error: null }),
}));
