// /chat/stream 请求构造：检索/重试常量与 `buildRequest` 纯函数。
//
// 独立成文件的原因：请求体构造只读 settingsStore，不写任何状态，属纯函数，
// 与 store 分离后可直接单测（现有用例即通过 mock chatStream 断言请求体）。

import type { ChatStreamRequest } from '@/types/ipc';
import { resolveDisplayModel, type ChatMessage } from '@/types/models';
import { useSettingsStore } from '@/stores/settingsStore';
import { buildHistory } from './messages';

/** 向量检索候选数。 */
const TOP_K = 20;
/** 重排后保留数。 */
const RERANK_TOP_K = 5;
/** P-04 自我纠正最大重试次数。 */
const MAX_RETRIES = 2;
/** 透传给查询改写的对话轮数（对齐 P-02 `HISTORY_TURNS=3`）。 */
const HISTORY_TURNS = 3;

/**
 * 构造 `/chat/stream` 请求体。
 *
 * `messages` 已包含刚发送的 user 消息，历史取其之前的最近 N 轮；
 * `embedding_model` 来自 settingsStore，`table_name` 对齐 LanceDB 命名
 * `documents_{embedding_model}_v{version}`。`fts_chunks` 恒空（Rust 侧
 * 尚无 chunk 级 FTS5，建索引为后续独立任务）。
 */
export function buildRequest(query: string, messages: ChatMessage[]): ChatStreamRequest {
  const settings = useSettingsStore.getState();
  const embeddingModel = settings.embeddingModel;
  // FE-m11：版本号从 embeddingModelOptions 查找，fallback 1（与 Rust 当前硬编码一致）
  const version =
    settings.embeddingModelOptions.find((m) => m.name === embeddingModel)?.version ?? 1;
  // 云端模式：若用户配置了 cloudModel，用它作为生成模型；否则按 provider 回落默认
  const effectiveLlmModel = resolveDisplayModel(
    settings.inferenceMode,
    settings.llmModel,
    settings.cloudModel,
    settings.cloudConsentProvider,
  );
  return {
    query,
    history: buildHistory(messages.slice(0, -1), HISTORY_TURNS),
    table_name: `documents_${embeddingModel}_v${version}`,
    embedding_model: embeddingModel,
    inference_mode: settings.inferenceMode.toLowerCase(),
    llm_model: effectiveLlmModel,
    // 云端模型通过 llm_model 传递（Rust chat_stream_inner 会合并 cloud_model → llm_model）
    // 此处直接传 effectiveLlmModel，云端 Provider 按前缀路由
    cloud_model: settings.inferenceMode === 'Cloud' ? settings.cloudModel : '',
    top_k: TOP_K,
    rerank_top_k: RERANK_TOP_K,
    max_retries: MAX_RETRIES,
    fts_chunks: [],
  };
}
