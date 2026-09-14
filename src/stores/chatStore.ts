// 聊天 store：对话历史 + RAG 流式响应（T6.6）。
//
// 流式链路：
//   sendMessage → chatStream(request)（Rust 代理 Sidecar /chat/stream）
//   Rust 后台逐帧解析 SSE → 经 chat://event 推送 → initChatListener 订阅
//   → handleChatEvent 按事件分发：
//     search_start → searching | search_result → searchInfo | token → 追加
//     retry → 清空缓冲重推（T5.7 契约）| citation → 暂存 | done → 组装消息 | error → 置错
//
// 持久化策略：messages 走 localStorage（关闭重开可看历史）；
// 流式状态（status/currentStream 等）为瞬态不持久化。
//
// 本文件只保留「状态 + 动作装配 + persist 配置」（complexity 规则：.ts 强制上限 250 行），
// 其余按职责落到 ./chat/ 子模块；对外导入路径 @/stores/chatStore 与公开符号保持不变。
//   ./chat/types.ts         公共类型、状态接口与初始状态
//   ./chat/messages.ts      消息 ID 与历史轮次工具
//   ./chat/request.ts       /chat/stream 请求构造与常量
//   ./chat/streamControl.ts seq 去重与看门狗（模块级瞬态）
//   ./chat/events.ts        chat://event 事件分发
//   ./chat/streaming.ts     发送 / 事件入口 / 追加片段 / 收尾
//   ./chat/listener.ts      chat://event 订阅
//   ./chat/lifecycle.ts     清空历史 / 清除错误

import { create } from 'zustand';
import { createJSONStorage, persist } from 'zustand/middleware';
import { throttledLocalStorage } from '../lib/throttledStorage';
import { createLifecycleActions } from './chat/lifecycle';
import { createListenerActions } from './chat/listener';
import { createStreamingActions } from './chat/streaming';
import { INITIAL_CHAT_STATE, type ChatState } from './chat/types';

export type { ChatSearchInfo, ChatStatus } from './chat/types';

export const useChatStore = create<ChatState>()(
  persist(
    (set, get) => ({
      ...INITIAL_CHAT_STATE,
      ...createStreamingActions({ set, get }),
      ...createListenerActions({ get }),
      ...createLifecycleActions({ set }),
    }),
    {
      name: 'filemind-chat',
      // 仅持久化 messages（流式状态不持久化）；
      // 写盘经 throttledLocalStorage 节流——流式期间每 token 一次 set，
      // 直写 localStorage 会放大为 token 级全量 stringify，终态在此处 flush 兜底
      storage: createJSONStorage(() => throttledLocalStorage),
      partialize: (state) => ({ messages: state.messages }),
    },
  ),
);
