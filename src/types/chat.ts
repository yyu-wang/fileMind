// 聊天相关前端模型：消息、RAG 引用与引用字面量清洗。
//
// 从 models.ts 拆出（该文件是前端模型统一入口，仍再导出本模块的符号，
// 54 处 `import ... from '@/types/models'` 无需改动）。

/** 聊天消息角色。 */
export enum ChatRole {
  User = 'user',
  Assistant = 'assistant',
  System = 'system',
}

/** RAG 引用（对齐 Sidecar citation 事件：id ⊆ search_result.sources.id）。 */
export interface ChatCitation {
  /** 引用来源 id（对应 search_result.sources.id） */
  id: number;
  /** 来源文件名 */
  fileName: string;
  /** 来源页码（前端预览定位依据） */
  page: number;
  /** 引用原文片段 */
  text: string;
}

/** 聊天消息（chatStore 使用）。 */
export interface ChatMessage {
  id: string;
  role: ChatRole;
  content: string;
  /** RAG 引用来源（T6.6 接入） */
  citations?: ChatCitation[];
  /** P-04 自我纠正：重试耗尽时标记低置信度 */
  lowConfidence?: boolean;
  /** P-04 自我纠正：实际重试次数 */
  retries?: number;
  createdAt: string;
}

/**
 * 与 Python 端 ``CITATION_PATTERN`` 语义一致的引用字面量正则。
 *
 * 用于前端 finishStream 中剥离正文中残留的引用标记（当 LLM 输出格式
 * 与解析器匹配失败时，作为最终视觉兜底）。
 * 兼容形式：``[1]`` / ``[引用1]`` / ``[引1]`` / ``[cite 2]`` / ``[来源3]`` 等。
 */
const CITATION_LITERAL_RE = /\[(?:引用|引|cite|citation|来源)?\s*(\d+)\s*\]/gi;

/**
 * 剥离正文中残留的引用字面量（[N]/[引用N] 等）。
 *
 * 引用已结构化存放在 ``ChatMessage.citations``，底部 chip 会渲染，
 * 正文中残留的 ``[引用1][引用2]`` 会是纯视觉噪音。stripCitationLiterals
 * 用于 ``finishStream`` 终态兜底：防止 Python 端漏解析（如 LLM 产生未知
 * 变体）时残留脏文本。
 *
 * @param content 原始回答正文（可能包含残留引用标记）
 * @returns 清理后的正文
 */
export function stripCitationLiterals(content: string): string {
  return (
    content
      .replace(CITATION_LITERAL_RE, '')
      // 去除引文标记后常见的残余间距：句号/问号/感叹号/逗号/分号/冒号前的空格
      .replace(/\s+([。？！，、；：,.!?;:])/g, '$1')
      .replace(/\s{2,}/g, ' ')
      .trim()
  );
}
