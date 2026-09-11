// Sidecar 生命周期 store（P1-1）：订阅 sidecar-status 事件 + 查询/重试命令。
//
// 状态瞬态（不持久化）：启动时 main.tsx 调 initListener + refreshStatus 对齐真值；
// 之后由 Rust 端 sidecar-status 事件（starting/ready/failed）驱动更新。
// 消费方：StatusBar（引擎状态胶囊）、ChatPage（未就绪禁用 AI 功能）。

import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { create } from 'zustand';
import { commands } from '../types/ipc';

/** Sidecar 运行时状态（与 Rust `SidecarStatus::name()` 对齐）。 */
export type SidecarRuntimeStatus = 'starting' | 'ready' | 'failed' | 'crash_loop';

/** Rust 端 `sidecar-status` 事件负载（`SidecarStatusEvent`，specta 未导出事件类型，本地对齐）。 */
interface SidecarStatusEventPayload {
  status: string;
  message: string | null;
}

/** 状态快照：事件负载 + 可选的重启计数（来自 `get_sidecar_status` 命令）。 */
type SidecarStatusSnapshot = SidecarStatusEventPayload & { restart_count?: number };

interface SidecarState {
  /** 引擎生命周期状态（默认 starting：后台引导线程通常尚未完成） */
  status: SidecarRuntimeStatus;
  /** 附加说明（failed 时的错误原因） */
  message: string | null;
  /** 历史累计重启次数（排障展示） */
  restartCount: number;
  /** 从 Rust 端查询最新状态（启动时对齐，防事件早于监听建立） */
  refreshStatus: () => Promise<void>;
  /** failed 状态下手动重试启动 */
  retryStart: () => Promise<void>;
  /** 处理 sidecar-status 事件 / 命令快照 */
  handleStatusEvent: (snapshot: SidecarStatusSnapshot) => void;
  /** 订阅 sidecar-status 事件（main.tsx 启动时调用，幂等） */
  initListener: () => Promise<void>;
}

/** 将 Rust 端字符串归一化为前端枚举（未知值兜底 crash_loop 便于排障）。 */
function toRuntimeStatus(raw: string): SidecarRuntimeStatus {
  if (raw === 'ready' || raw === 'starting' || raw === 'failed') {
    return raw;
  }
  return 'crash_loop';
}

// 模块级缓存：订阅一次性建立，避免 HMR / 重复调用产生多份监听
let sidecarUnlisten: UnlistenFn | null = null;

export const useSidecarStore = create<SidecarState>()((set, get) => ({
  status: 'starting',
  message: null,
  restartCount: 0,

  refreshStatus: async () => {
    try {
      const result = await commands.getSidecarStatus();
      if (result.status === 'ok') {
        get().handleStatusEvent(result.data);
      } else {
        console.warn('[sidecar] 状态查询失败:', result.error);
      }
    } catch (e) {
      console.warn('[sidecar] 状态查询异常:', e);
    }
  },

  retryStart: async () => {
    const result = await commands.retrySidecarStart();
    if (result.status === 'ok') {
      // 乐观置为 starting：后台引导线程随后发事件校正（成功→ready / 失败→failed）
      set({ status: 'starting', message: null });
    } else {
      set({ message: result.error });
    }
  },

  handleStatusEvent: (snapshot) => {
    set((state) => ({
      status: toRuntimeStatus(snapshot.status),
      message: snapshot.message,
      restartCount: snapshot.restart_count ?? state.restartCount,
    }));
  },

  initListener: async () => {
    // FE-m9：await 前占位，防止并发双调用都通过幂等检查后双注册
    if (sidecarUnlisten) return;
    sidecarUnlisten = () => {};
    try {
      sidecarUnlisten = await listen<SidecarStatusEventPayload>('sidecar-status', (event) => {
        get().handleStatusEvent(event.payload);
      });
    } catch (err) {
      // 注册失败：清掉占位允许重试
      sidecarUnlisten = null;
      console.error('[sidecar] 事件订阅失败', err);
    }
  },
}));
