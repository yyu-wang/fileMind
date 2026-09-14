// 执行生命周期动作：暂停 / 继续 / 取消 / 复位 / 清除错误。
//
// 与 store 分离的原因：这五个动作只做「开关 + 状态机校验」，不碰 IPC，也不参与
// 预览与执行编排；单独成模块后 store 只需把依赖（set/status/control）注入进来。
//
// 依赖通过 `LifecycleDeps` 注入而非直接引用 store：避免子模块反向 import store
// 造成循环依赖，也让这些动作可以在测试里脱离 store 单独驱动。

import { ClassifyStatus } from '@/types/models';
import type { ExecControl } from './executor';
import { INITIAL_PROGRESS, type ClassifyState } from './types';

/** 生命周期动作所需的最小依赖面。 */
export interface LifecycleDeps {
  /** 写入状态（zustand 的 set） */
  set: (patch: Partial<ClassifyState>) => void;
  /** 读当前状态机（zustand 的 get().status） */
  status: () => ClassifyStatus;
  /** 执行循环的非响应式开关（暂停/取消由它中断 chunk 循环） */
  control: ExecControl;
  /** 作废在途的执行与预览请求（递增执行令牌与预览序号） */
  invalidateInFlight: () => void;
}

/**
 * 生成生命周期动作集合（供 store 展开进 create 的返回对象）。
 *
 * Args:
 *   deps: store 注入的依赖面
 *
 * Returns:
 *   暂停/继续/取消/复位/清错五个动作
 */
export function createLifecycleActions(
  deps: LifecycleDeps,
): Pick<ClassifyState, 'pause' | 'resume' | 'cancel' | 'reset' | 'clearError'> {
  const { set, status, control, invalidateInFlight } = deps;
  return {
    pause: () => {
      if (status() !== ClassifyStatus.Running) return;
      control.paused = true;
      set({ status: ClassifyStatus.Paused });
    },

    resume: () => {
      if (status() !== ClassifyStatus.Paused) return;
      control.paused = false;
      set({ status: ClassifyStatus.Running });
    },

    cancel: () => {
      // FE-m7：Idle/Done 态误调 cancel 会污染状态机（直接跳到 Cancelled）
      const current = status();
      if (current !== ClassifyStatus.Running && current !== ClassifyStatus.Paused) {
        return;
      }
      control.cancelled = true;
      set({ status: ClassifyStatus.Cancelled });
    },

    reset: () => {
      // FE-C5 / FE-M9：作废在途执行与预览——执行循环恢复后发现令牌失配即静默退出，
      // 不再把状态覆盖回 Cancelled/Done，也不会带回旧的 execSummary。
      invalidateInFlight();
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
  };
}
