// 聊天事件订阅动作：建立 chat://event 的一次性监听（幂等）。
//
// 与 store 分离的原因：模块级 unlisten 句柄与订阅动作自成一体，
// store 只需把读状态依赖（get）注入进来。

import { listenChatEvent } from '@/lib/ipc/chatIpc';
import type { ChatGet, ChatState } from './types';

// 模块级缓存：订阅一次性建立，避免 HMR / 重复调用产生多份监听
let chatUnlisten: (() => void) | null = null;

/**
 * 生成事件订阅动作集合（供 store 展开进 create 的返回对象）。
 *
 * Args:
 *   deps: store 注入的 get（用于把事件转交 handleChatEvent）
 *
 * Returns:
 *   initChatListener 动作
 */
export function createListenerActions({
  get,
}: {
  get: ChatGet;
}): Pick<ChatState, 'initChatListener'> {
  return {
    initChatListener: async () => {
      // FE-m9：await 前占位，防止并发双调用都通过幂等检查后双注册
      if (chatUnlisten) return;
      chatUnlisten = () => {}; // 占位：后续 await 期间第二次调用会命中上行 if 直接返回
      try {
        const unlisten = await listenChatEvent((event) => get().handleChatEvent(event));
        chatUnlisten = unlisten;
      } catch (err) {
        // 注册失败：清掉占位允许重试
        chatUnlisten = null;
        console.error('chat listener 注册失败', err);
      }
    },
  };
}
