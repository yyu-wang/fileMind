// 分类 store：分类预览、分块执行、暂停/取消、整批撤销。
//
// 状态机（ClassifyStatus）：
//   Idle → Previewing → Running ⇄ Paused → Done | Cancelled
// 预览态用 `preview != null` 区分（预览成功后 status 回到 Idle，页面渲染预览树）。
//
// 执行复用 T3.x `execute_operations`（每项带各自 `new_path` 的 PlanItem + Skip 冲突策略），
// 共享链式撤销；成功项再逐项调 `update_file_category` 打分类标签。

import { create } from 'zustand';
import { fileIpc } from '../lib/ipc';
import type { ClassifyPlanItem, ClassifyPreview, PlanItem } from '../types/ipc';
import { ClassifyStatus } from '../types/models';
import { useFileStore } from './fileStore';

/** 待确认分组的展示名（对应 Rust `pending` 来源标签）。 */
export const PENDING_NAME = '待确认';

/** 单块执行的文件数上限（分批调用避免单次 IPC 过久）。 */
const CHUNK_SIZE = 50;

interface ProgressState {
  /** 已完成数 */
  done: number;
  /** 总数 */
  total: number;
}

/** 执行结果汇总（Done/Cancelled 面板展示）。 */
export interface ClassifyExecSummary {
  /** 实际执行成功数 */
  success: number;
  /** 实际执行失败数 */
  failed: number;
  /** 待确认文件数 */
  pending: number;
  /** 预览文件总数 */
  total: number;
}

/** 执行循环控制（非响应式：暂停/取消通过它中断 chunk 循环）。 */
interface ExecControl {
  paused: boolean;
  cancelled: boolean;
}

const control: ExecControl = { paused: false, cancelled: false };

const INITIAL_PROGRESS: ProgressState = { done: 0, total: 0 };

interface ClassifyState {
  /** 当前分类流程状态 */
  status: ClassifyStatus;
  /** 预览结果 */
  preview: ClassifyPreview | null;
  /** 待确认文件的 id 列表（执行时排除） */
  pendingIds: string[];
  /** 执行进度 */
  progress: ProgressState;
  /** 执行结果汇总 */
  execSummary: ClassifyExecSummary | null;
  /** 最近批次 ID（用于撤销） */
  lastBatchId: string | null;
  /** 错误信息 */
  error: string | null;

  /** 生成分类预览（scanPath 从 fileStore 读） */
  generatePreview: (fileIds: string[]) => Promise<void>;
  /** 分块执行已确认分类（不含待确认/冲突项） */
  execute: () => Promise<void>;
  /** 暂停执行 */
  pause: () => void;
  /** 继续执行 */
  resume: () => void;
  /** 取消执行（已执行块保留，可手动撤销整批） */
  cancel: () => void;
  /** 撤销最近整批 */
  undoLastBatch: () => Promise<void>;
  /** 重置到初始状态 */
  reset: () => void;
  /** 清除错误 */
  clearError: () => void;
}

/** 分类计划项 → T3.x 执行用 PlanItem（Move 到各自目标路径）。 */
function toPlanItem(item: ClassifyPlanItem): PlanItem {
  return {
    file_id: item.file_id,
    file_name: item.file_name,
    original_path: item.original_path,
    new_path: item.target_path,
    operation: 'Move',
    status: item.status,
    conflict_type: item.conflict_type,
  };
}

/** 暂停期间阻塞当前 chunk；返回时是否已被取消。 */
async function waitWhilePaused(): Promise<boolean> {
  while (control.paused && !control.cancelled) {
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  return control.cancelled;
}

/** 汇总执行结果（pending/total 来自预览，success/failed 来自实际执行）。 */
function buildSummary(
  preview: ClassifyPreview,
  success: number,
  failed: number,
): ClassifyExecSummary {
  return {
    success,
    failed,
    pending: preview.items.filter((item) => item.category_name == null).length,
    total: preview.items.length,
  };
}

export const useClassifyStore = create<ClassifyState>()((set, get) => ({
  status: ClassifyStatus.Idle,
  preview: null,
  pendingIds: [],
  progress: INITIAL_PROGRESS,
  execSummary: null,
  lastBatchId: null,
  error: null,

  generatePreview: async (fileIds) => {
    const scanPath = useFileStore.getState().scanPath;
    if (!scanPath) {
      set({ status: ClassifyStatus.Idle, error: '请先在文件页选择要整理的目录' });
      return;
    }
    set({ status: ClassifyStatus.Previewing, error: null, preview: null, execSummary: null });
    const result = await fileIpc.classifyPreview(fileIds, scanPath);
    if (result.status === 'ok') {
      const pendingIds = result.data.items
        .filter((item) => item.category_name == null)
        .map((item) => item.file_id);
      set({ status: ClassifyStatus.Idle, preview: result.data, pendingIds });
    } else {
      set({ status: ClassifyStatus.Idle, error: result.error });
    }
  },

  execute: async () => {
    const preview = get().preview;
    if (!preview) {
      set({ status: ClassifyStatus.Idle, error: '尚未生成分类预览' });
      return;
    }
    const execItems = preview.items.filter(
      (item) => item.category_name != null && item.status === 'Ok',
    );
    if (execItems.length === 0) {
      set({ status: ClassifyStatus.Done, execSummary: buildSummary(preview, 0, 0) });
      return;
    }

    const plan = execItems.map(toPlanItem);
    control.paused = false;
    control.cancelled = false;
    set({ status: ClassifyStatus.Running, error: null, progress: { done: 0, total: plan.length } });

    let success = 0;
    let failed = 0;
    let lastBatchId: string | null = null;

    for (let i = 0; i < plan.length; i += CHUNK_SIZE) {
      if (await waitWhilePaused()) break;
      const chunk = plan.slice(i, i + CHUNK_SIZE);
      const result = await fileIpc.executeOperations({
        batch_id: preview.batch_id,
        plan: chunk,
        exclude_file_ids: [],
      });
      if (result.status === 'error') {
        failed += chunk.length;
        set({ error: result.error });
        break;
      }
      for (const r of result.data.results) {
        if (r.success) {
          success += 1;
          const execItem = execItems.find((item) => item.file_id === r.file_id);
          if (execItem?.category_name) {
            await fileIpc.updateFileCategory(r.file_id, execItem.category_name);
          }
        } else {
          failed += 1;
        }
      }
      lastBatchId = result.data.batch_id;
      set({ progress: { done: success + failed, total: plan.length }, lastBatchId });
    }

    const nextStatus = control.cancelled ? ClassifyStatus.Cancelled : ClassifyStatus.Done;
    set({ status: nextStatus, execSummary: buildSummary(preview, success, failed) });
    // 移动/打标已落库，刷新文件页列表（幂等，失败不影响本次状态）
    void useFileStore.getState().loadAllFiles();
  },

  pause: () => {
    if (get().status === ClassifyStatus.Running) {
      control.paused = true;
      set({ status: ClassifyStatus.Paused });
    }
  },

  resume: () => {
    if (get().status === ClassifyStatus.Paused) {
      control.paused = false;
      set({ status: ClassifyStatus.Running });
    }
  },

  cancel: () => {
    control.cancelled = true;
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
      set({ status: ClassifyStatus.Idle, lastBatchId: null, progress: INITIAL_PROGRESS });
      void useFileStore.getState().loadAllFiles();
    } else {
      set({ error: result.error });
    }
  },

  reset: () => {
    control.cancelled = true;
    control.paused = false;
    set({
      status: ClassifyStatus.Idle,
      preview: null,
      pendingIds: [],
      progress: INITIAL_PROGRESS,
      execSummary: null,
      lastBatchId: null,
      error: null,
    });
  },

  clearError: () => set({ error: null }),
}));
