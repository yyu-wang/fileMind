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
import type { Category, ClassifyPlanItem, ClassifyPreview, PlanItem } from '../types/ipc';
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
  /** 全部分类（T6.12 手动分类下拉数据源，进入预览时加载缓存） */
  categories: Category[];
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
  /** 加载分类列表（幂等，供手动分类下拉使用） */
  loadCategories: () => Promise<void>;
  /** 手动指定待确认文件的分类（T6.12：更新 preview，执行时随主批量移动+打标） */
  assignCategory: (fileId: string, category: Category) => void;
  /** 批量手动指定分类（T6.12 增强：一次 set 更新多个文件，避免逐点过慢） */
  assignCategories: (fileIds: string[], category: Category) => void;
  /** 分块执行已确认分类；`resolveConflicts=true` 时冲突项按 Rename 策略一并执行（确认执行全部） */
  execute: (resolveConflicts?: boolean) => Promise<void>;
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

/**
 * 拼接目标路径：`scanPath/targetDir/fileName`（合并重复斜杠）。
 *
 * T6.12 手动分类用：分类的 `target_dir` 是相对扫描根的子目录，
 * 与 Rust `build_target_path` 的拼接语义一致。
 */
function joinPath(...parts: string[]): string {
  return parts
    .filter((p) => p && p.trim() !== '')
    .join('/')
    .replace(/\/{2,}/g, '/');
}

/** 校验分类目标目录：拒绝绝对路径 / 路径穿越（与 Rust validate_relative_subpath 语义对齐）。 */
function isSafeTargetDir(targetDir: string): boolean {
  if (!targetDir || targetDir.trim() === '') return false;
  return !targetDir.includes('..') && !targetDir.startsWith('/') && !/^[A-Za-z]:/.test(targetDir);
}

export const useClassifyStore = create<ClassifyState>()((set, get) => ({
  status: ClassifyStatus.Idle,
  preview: null,
  pendingIds: [],
  categories: [],
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
      // T6.12：加载分类列表供手动分类下拉使用（幂等，失败不影响预览）
      void get().loadCategories();
    } else {
      set({ status: ClassifyStatus.Idle, error: result.error });
    }
  },

  loadCategories: async () => {
    // 幂等：已有缓存不重复拉取（进入预览时调用一次即可）
    if (get().categories.length > 0) return;
    const result = await fileIpc.listCategories();
    if (result.status === 'ok') {
      set({ categories: result.data });
    } else {
      set({ error: result.error });
    }
  },

  assignCategory: (fileId, category) => {
    const { preview, pendingIds } = get();
    if (!preview) return;
    const item = preview.items.find((i) => i.file_id === fileId);
    if (!item) return;

    // 目标目录合法性校验（相对路径、无穿越），非法拒绝并提示
    const targetDir = category.target_dir.trim();
    if (!isSafeTargetDir(targetDir)) {
      set({ error: `分类「${category.name}」未配置有效目标目录，无法手动分类` });
      return;
    }
    const scanPath = useFileStore.getState().scanPath;
    if (!scanPath) {
      set({ error: '请先在文件页选择要整理的目录' });
      return;
    }

    // 计算目标路径（与 Rust build_target_path 拼接语义一致），更新 preview 项
    const targetPath = joinPath(scanPath, targetDir, item.file_name);
    const updatedItems = preview.items.map((i) =>
      i.file_id === fileId
        ? { ...i, category_name: category.name, rule_source: 'manual', target_path: targetPath }
        : i,
    );
    const stats = {
      ...preview.stats,
      categorized: preview.stats.categorized + 1,
      pending: Math.max(preview.stats.pending - 1, 0),
    };
    set({
      preview: { ...preview, items: updatedItems, stats },
      pendingIds: pendingIds.filter((id) => id !== fileId),
      error: null,
    });
  },

  assignCategories: (fileIds, category) => {
    const { preview, pendingIds } = get();
    if (!preview || fileIds.length === 0) return;

    // 批量共用同一分类：一次校验 target_dir（相对路径、无穿越）
    const targetDir = category.target_dir.trim();
    if (!isSafeTargetDir(targetDir)) {
      set({ error: `分类「${category.name}」未配置有效目标目录，无法手动分类` });
      return;
    }
    const scanPath = useFileStore.getState().scanPath;
    if (!scanPath) {
      set({ error: '请先在文件页选择要整理的目录' });
      return;
    }

    const idSet = new Set(fileIds);
    let assigned = 0;
    const updatedItems = preview.items.map((i) => {
      if (!idSet.has(i.file_id)) return i;
      // 只处理未分类项；已分类的跳过（避免重复计数/覆盖）
      if (i.category_name != null) return i;
      assigned += 1;
      const targetPath = joinPath(scanPath, targetDir, i.file_name);
      return {
        ...i,
        category_name: category.name,
        rule_source: 'manual',
        target_path: targetPath,
      };
    });

    if (assigned === 0) return;
    set({
      preview: {
        ...preview,
        items: updatedItems,
        stats: {
          ...preview.stats,
          categorized: preview.stats.categorized + assigned,
          pending: Math.max(preview.stats.pending - assigned, 0),
        },
      },
      pendingIds: pendingIds.filter((id) => !idSet.has(id)),
      error: null,
    });
  },

  execute: async (resolveConflicts = false) => {
    const preview = get().preview;
    if (!preview) {
      set({ status: ClassifyStatus.Idle, error: '尚未生成分类预览' });
      return;
    }
    // resolveConflicts=true（确认执行全部）：冲突项也纳入 plan，由 Rust 按 Rename 重算执行
    const execItems = preview.items.filter(
      (item) => item.category_name != null && (resolveConflicts || item.status === 'Ok'),
    );
    if (execItems.length === 0) {
      // 本次无可执行项：全部待确认（未分类）或已分类但目标冲突。
      // 保留 lastBatchId（上次批次仍可撤销），并给出原因提示避免困惑。
      const hasCategorized = preview.items.some((i) => i.category_name != null);
      set({
        status: ClassifyStatus.Done,
        execSummary: buildSummary(preview, 0, 0),
        error: hasCategorized ? '没有可执行的分类项（文件已分类或目标冲突）' : null,
      });
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
        resolve_conflicts: resolveConflicts,
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
    // 移动/打标已落库：只刷新轻量统计，不拉全量文件列表——
    // 文件库可达数十万级，全量刷新（loadAllFiles）会卡死 UI；列表由用户手动「刷新」。
    void useFileStore.getState().loadStats();
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
      // 同 execute：撤销后只刷统计，避免数十万级全量刷新卡 UI
      void useFileStore.getState().loadStats();
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
