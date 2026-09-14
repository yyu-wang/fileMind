// 聊天消息工具：临时 ID 生成与历史轮次提取（纯函数，便于单测）。
//
// 独立成文件的原因：`buildHistory` 被请求构造（request.ts）复用，`genMessageId`
// 被发送与收尾（streaming.ts）复用；抽到独立模块避免 request 与 streaming 互相依赖。

import type { ChatTurn } from '@/types/ipc';
import { ChatRole, type ChatMessage } from '@/types/models';

/** 生成消息 ID（前端临时 ID，非持久化主键）。 */
export function genMessageId(): string {
  return `msg-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
}

/** 取最近 `maxTurns` 轮 user/assistant 对话对（P-02 查询改写输入）。 */
export function buildHistory(messages: ChatMessage[], maxTurns: number): ChatTurn[] {
  const turns: ChatTurn[] = [];
  for (let i = messages.length - 1; i >= 0; i--) {
    const assistant = messages[i];
    if (assistant.role !== ChatRole.Assistant) continue;
    const user = messages[i - 1];
    if (!user || user.role !== ChatRole.User) continue;
    turns.push({ user: user.content, assistant: assistant.content });
    i--;
    if (turns.length >= maxTurns) break;
  }
  return turns.reverse();
}
