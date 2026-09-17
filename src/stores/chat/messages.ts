// 聊天消息工具：临时 ID 生成、消息构造与历史轮次提取（纯函数，便于单测）。
//
// 独立成文件的原因：`buildHistory` 被请求构造（request.ts）复用，`genMessageId` /
// 消息构造被发送与收尾（streaming.ts）复用；抽到独立模块避免 request 与 streaming
// 互相依赖。

import type { ChatTurn } from '@/types/ipc';
import {
  ChatRole,
  stripCitationLiterals,
  type ChatCitation,
  type ChatMessage,
} from '@/types/models';

/** 生成消息 ID（前端临时 ID，非持久化主键）。 */
export function genMessageId(): string {
  return `msg-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
}

/** 构造用户消息（发送动作的起点）。 */
export function buildUserMessage(content: string): ChatMessage {
  return {
    id: genMessageId(),
    role: ChatRole.User,
    content,
    createdAt: new Date().toISOString(),
  };
}

/**
 * 构造助手消息（流式收尾组装）。
 *
 * 正文经 stripCitationLiterals 清洗（Python 端漏解析时残留的 `[引用N]` 是纯视觉噪音，
 * 引用已结构化在 citations 里）；可选字段仅在非空时带上，避免落库里出现
 * `lowConfidence: false` / `retries: 0` 这类噪音字段。
 */
export function buildAssistantMessage(args: {
  content: string;
  citations: ChatCitation[];
  retries: number;
  lowConfidence: boolean;
}): ChatMessage {
  const { content, citations, retries, lowConfidence } = args;
  return {
    id: genMessageId(),
    role: ChatRole.Assistant,
    content: stripCitationLiterals(content),
    ...(citations.length > 0 ? { citations } : {}),
    ...(lowConfidence ? { lowConfidence: true } : {}),
    ...(retries > 0 ? { retries } : {}),
    createdAt: new Date().toISOString(),
  };
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
