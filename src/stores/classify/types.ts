// 分类流程的公共类型与常量：供 store 与同目录子模块共用。
//
// 独立成文件的原因：`ClassifyState` 与 `ClassifyExecSummary` 等类型被 store、
// executor、manualAssign 三方引用，放在 store 里会让子模块反向依赖 store
// （形成循环导入）。

import type { Category, ClassifyPreview } from '@/types/ipc';
import type { ClassifyStatus } from '@/types/models';

/** 待确认分组的展示名（对应 Rust `pending` 来源标签）。 */
export const PENDING_NAME = '待确认';

/**
 * 分类执行方式：
 *   `move` 移动原文件到同级收纳目录 `<扫描目录名>_已分类` 的分类子文件夹；
 *   `copy` 保留原文件，复制副本到同级收纳目录的分类子文件夹（不影响原文件）。
 * 后端 `execute_operations` 原生支持 Copy，纯前端传 operation 即可。
 */
export type ClassifyExecMode = 'move' | 'copy';

/** 执行进度。 */
export interface ProgressState {
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

/** 分类 store 的状态与动作集合。 */
export interface ClassifyState {
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
  /** 强制重拉分类列表（ruleStore 增删分类后调用，使幂等缓存失效） */
  refreshCategories: () => Promise<void>;
  /** 手动指定待确认文件的分类（T6.12：更新 preview，执行时随主批量移动+打标） */
  assignCategory: (fileId: string, category: Category) => void;
  /** 批量手动指定分类（T6.12 增强：一次 set 更新多个文件，避免逐点过慢） */
  assignCategories: (fileIds: string[], category: Category) => void;
  /** 分块执行已确认分类；`resolveConflicts=true` 时冲突项按 Rename 策略一并执行（确认执行全部）；`mode` 决定移动还是复制 */
  execute: (resolveConflicts?: boolean, mode?: ClassifyExecMode) => Promise<void>;
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

/** 初始进度（reset 与未开始执行时复用同一对象语义）。 */
export const INITIAL_PROGRESS: ProgressState = { done: 0, total: 0 };
