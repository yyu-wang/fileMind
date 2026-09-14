// 分类 store：分类预览、分块执行、暂停/取消、整批撤销。
//
// 状态机（ClassifyStatus）：
//   Idle → Previewing → Running ⇄ Paused → Done | Cancelled
// 预览态用 `preview != null` 区分（预览成功后 status 回到 Idle，页面渲染预览树）。
//
// 执行复用 T3.x `execute_operations`（每项带各自 `new_path` 的 PlanItem + Skip 冲突策略），
// 共享链式撤销；成功项再逐项调 `update_file_category` 打分类标签。
//
// 本文件只保留「状态 + 预览/执行编排」（complexity 规则：.ts 强制上限 250 行），
// 其余按职责落到 ./classify/ 子模块；对外导入路径 @/stores/classifyStore 与
// 公开符号保持不变。
//   ./classify/types.ts         公共类型与常量
//   ./classify/paths.ts         目标路径拼接与校验
//   ./classify/errors.ts        IPC 异常消息归一化
//   ./classify/executor.ts      分批执行引擎（含结果汇总）
//   ./classify/manualAssign.ts  手动分类的预览重算与状态映射
//   ./classify/lifecycle.ts     暂停/继续/取消/复位/清错

import { create } from 'zustand';
import { fileIpc } from '../lib/ipc';
import { ClassifyStatus } from '../types/models';
import { ipcErrorMessage } from './classify/errors';
import {
  buildSummary,
  runBatchedExecution,
  toPlanItem,
  type ExecControl,
} from './classify/executor';
import { createLifecycleActions } from './classify/lifecycle';
import { computeManualAssign, manualOutcomeToState } from './classify/manualAssign';
import { INITIAL_PROGRESS, type ClassifyState } from './classify/types';
import { useFileStore } from './fileStore';

export { PENDING_NAME } from './classify/types';
export type { ClassifyExecMode, ClassifyExecSummary } from './classify/types';

/** 执行循环控制（非响应式：暂停/取消通过它中断 chunk 循环）。 */
const control: ExecControl = { paused: false, cancelled: false };

/**
 * 执行令牌（FE-C5）：每次 execute 递增取号，reset() 再递增作废在途执行。
 * 在途执行在每个 await 恢复点校验令牌，失配即放弃写状态——
 * 防止「用户已 reset 重来」后被旧循环的收尾 set 覆盖回 Cancelled/Done。
 */
let execToken = 0;

// FE-M9：预览请求序号——生成中的慢响应被后续请求/取消作废，
// 到达后序号失配直接丢弃，防止旧预览覆盖新状态。
let previewSeq = 0;

export const useClassifyStore = create<ClassifyState>()((set, get) => ({
  status: ClassifyStatus.Idle,
  preview: null,
  pendingIds: [],
  categories: [],
  progress: INITIAL_PROGRESS,
  execSummary: null,
  lastBatchId: null,
  error: null,

  ...createLifecycleActions({
    set,
    status: () => get().status,
    control,
    invalidateInFlight: () => {
      execToken += 1;
      previewSeq += 1;
    },
  }),

  generatePreview: async (fileIds) => {
    const scanPath = useFileStore.getState().scanPath;
    if (!scanPath) {
      set({ status: ClassifyStatus.Idle, error: '请先在文件页选择要整理的目录' });
      return;
    }
    // FE-M9：重入守卫——StrictMode 双挂载/用户快速连点时，进行中的预览
    // 直接忽略后续调用（旧实现并发跑两个 IPC，晚回者覆盖早回者）
    if (get().status === ClassifyStatus.Previewing) return;
    const seq = ++previewSeq;
    set({ status: ClassifyStatus.Previewing, error: null, preview: null, execSummary: null });
    try {
      const result = await fileIpc.classifyPreview(fileIds, scanPath);
      // FE-M9：期间有新请求发起或 reset 被调用，本次响应已过期，丢弃
      if (seq !== previewSeq) return;
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
    } catch (err) {
      // 必须复位：status 卡在 Previewing 会让上面的重入守卫永久拒绝后续预览，
      // 分类页彻底不可用（按钮一直显示生成中）
      if (seq === previewSeq) set({ status: ClassifyStatus.Idle, error: ipcErrorMessage(err) });
    }
  },

  loadCategories: async () => {
    // 幂等：已有缓存不重复拉取（进入预览时调用一次即可）
    if (get().categories.length > 0) return;
    try {
      const result = await fileIpc.listCategories();
      if (result.status === 'ok') {
        set({ categories: result.data });
      } else {
        set({ error: result.error });
      }
    } catch (err) {
      // 调用方是 `void get().loadCategories()`，抛出去只会变成 unhandled rejection
      set({ error: ipcErrorMessage(err) });
    }
  },

  refreshCategories: async () => {
    // 绕过幂等缓存强制重拉。失败保持旧缓存静默返回：调用方（规则页增删分类）
    // 已有自己的成功/失败提示，此处再 set error 会把规则页操作误报为分类页错误。
    try {
      const result = await fileIpc.listCategories();
      if (result.status === 'ok') {
        set({ categories: result.data });
      }
    } catch (err) {
      // 同「静默返回」契约：只记日志，不污染分类页错误横幅
      console.warn('[classify] 刷新分类缓存失败:', err);
    }
  },

  assignCategory: (fileId, category) => {
    const { preview, pendingIds } = get();
    if (!preview) return;
    const item = preview.items.find((i) => i.file_id === fileId);
    // FE-M1：与批量版 assignCategories 对齐——已分类项拒绝覆盖，
    // 避免重复调用导致分类被覆盖、stats.categorized 虚增、pending 重复扣减
    if (!item || item.category_name != null) return;
    const outcome = computeManualAssign({
      preview,
      fileIds: [fileId],
      category,
      outputRoot: preview.output_root || useFileStore.getState().scanPath || null,
    });
    const patch = manualOutcomeToState(outcome, pendingIds);
    if (patch) set(patch);
  },

  assignCategories: (fileIds, category) => {
    const { preview, pendingIds } = get();
    if (!preview || fileIds.length === 0) return;
    const outcome = computeManualAssign({
      preview,
      fileIds,
      category,
      outputRoot: preview.output_root || useFileStore.getState().scanPath || null,
    });
    const patch = manualOutcomeToState(outcome, pendingIds);
    if (patch) set(patch);
  },

  execute: async (resolveConflicts = false, mode = 'move') => {
    // FE-C1 重入守卫：执行中/暂停中拒绝再次进入，防止双执行循环交错
    // 重复移动文件、operations_log 双写污染撤销链。
    if (get().status === ClassifyStatus.Running || get().status === ClassifyStatus.Paused) {
      return;
    }
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

    const plan = execItems.map((item) => toPlanItem(item, mode));
    const token = ++execToken;
    control.paused = false;
    control.cancelled = false;
    set({ status: ClassifyStatus.Running, error: null, progress: { done: 0, total: plan.length } });
    /** 当前执行是否仍持有令牌（reset 会作废在途执行的写状态权利）。 */
    const stillHolds = () => token === execToken;

    // 执行循环与异常收尾都在 runBatchedExecution 内：它把「已完成的部分结果」
    // 一并带回，store 不会因为异常边界拿不到 success/failed（那样只能按 0/0 收尾）。
    const outcome = await runBatchedExecution({
      plan,
      batchId: preview.batch_id,
      execItems,
      resolveConflicts,
      control,
      stillHolds,
      onProgress: (done, lastBatchId) =>
        set({ progress: { done, total: plan.length }, lastBatchId }),
      onError: (message) => set({ error: message }),
    });
    if (outcome.abandoned) return;
    // 异常中断按「取消」语义收尾：保留已完成块与整批撤销入口，
    // execSummary 呈现真实的部分结果（而非 0/0）。
    if (outcome.aborted) control.cancelled = true;
    set({
      status: control.cancelled ? ClassifyStatus.Cancelled : ClassifyStatus.Done,
      execSummary: buildSummary(preview, outcome.success, outcome.failed),
    });
    // 移动/打标已落库：只刷新轻量统计，不拉全量文件列表——
    // 文件库可达数十万级，全量刷新（loadAllFiles）会卡死 UI；列表由用户手动「刷新」。
    // 异常中断同样可能已移动部分文件，故与正常路径一致地刷新。
    void useFileStore.getState().loadStats();
  },

  undoLastBatch: async () => {
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
  },
}));
