// 全局设置 store：推理模式、模型名、数据目录、云端同意书状态。
//
// 持久化策略：inferenceMode + cloudConsentSigned 走 localStorage（快速启动显示），
// 启动后 main.tsx 调 loadConfig() 从 Rust 端校正真值
//
// 设计原则（03 设计稿 1.1 隐私可见）：
// - 推理模式切换需用户主动操作，不静默
// - MODE_SWITCH_FORBIDDEN 错误码是安全阀门（04 API §2-3）

import { create } from 'zustand';
import { persist } from 'zustand/middleware';
import { fileIpc } from '../lib/ipc';
import type { AppConfig, InferenceMode } from '../types/ipc';

interface SettingsState {
  /** 当前推理模式（默认本地） */
  inferenceMode: InferenceMode;
  /** 当前模型显示名（状态栏用） */
  llmModel: string;
  /** 数据目录 */
  dataDirectory: string;
  /** 本地 Embedding 模型名 */
  embeddingModel: string;
  /** 最大单文件大小（MB） */
  maxFileSizeMb: number;
  /** 界面语言（BCP 47） */
  language: string;
  /** 云端同意书是否已签（云端模式前置条件） */
  cloudConsentSigned: boolean;
  /** 配置加载中 */
  isLoading: boolean;
  /** 错误信息（null 表示无错误） */
  error: string | null;

  /** 从 Rust 端加载完整配置（启动时调用） */
  loadConfig: () => Promise<void>;
  /** 切换推理模式（需用户主动调用，记录审计日志） */
  setInferenceMode: (mode: InferenceMode) => Promise<void>;
  /** 更新配置（部分字段） */
  updateConfig: (partial: Partial<AppConfig>) => Promise<void>;
  /** 签署云端同意书（T6.7 实现） */
  signCloudConsent: () => Promise<void>;
  /** 撤销云端同意书（T6.7 实现） */
  revokeCloudConsent: () => Promise<void>;
  /** 清除错误 */
  clearError: () => void;
}

export const useSettingsStore = create<SettingsState>()(
  persist(
    (set, get) => ({
      inferenceMode: 'Local',
      llmModel: 'Ollama · Qwen2.5:7B',
      dataDirectory: '',
      embeddingModel: 'bge-small-zh',
      maxFileSizeMb: 100,
      language: 'zh-CN',
      cloudConsentSigned: false,
      isLoading: true,
      error: null,

      loadConfig: async () => {
        set({ isLoading: true, error: null });
        const result = await fileIpc.getConfig();
        if (result.status === 'ok') {
          const cfg = result.data;
          set({
            dataDirectory: cfg.data_directory,
            inferenceMode: normalizeInferenceMode(cfg.inference_mode),
            embeddingModel: cfg.embedding_model,
            maxFileSizeMb: cfg.max_file_size_mb,
            language: cfg.language,
            llmModel: getLlmModelLabel(normalizeInferenceMode(cfg.inference_mode)),
            isLoading: false,
          });
        } else {
          set({ isLoading: false, error: result.error });
        }
      },

      setInferenceMode: async (mode) => {
        // source='ui' 标识来自前端用户主动操作（04 API §2-3b 安全约束）
        const result = await fileIpc.setInferenceMode(mode, 'ui');
        if (result.status === 'ok') {
          set({ inferenceMode: mode, llmModel: getLlmModelLabel(mode) });
        } else {
          set({ error: result.error });
          throw new Error(result.error);
        }
      },

      updateConfig: async (partial) => {
        // Rust 端要求完整 AppConfig，先合并当前值再发送
        const current = get();
        const fullConfig: AppConfig = {
          data_directory: partial.data_directory ?? current.dataDirectory,
          inference_mode: partial.inference_mode ?? current.inferenceMode.toLowerCase(),
          embedding_model: partial.embedding_model ?? current.embeddingModel,
          max_file_size_mb: partial.max_file_size_mb ?? current.maxFileSizeMb,
          language: partial.language ?? current.language,
        };
        const result = await fileIpc.updateConfig(fullConfig);
        if (result.status !== 'ok') {
          set({ error: result.error });
          throw new Error(result.error);
        }
        // 成功后重新加载完整配置，保证状态一致
        await get().loadConfig();
      },

      signCloudConsent: async () => {
        // T6.7 实现：调用 fileIpc.signCloudConsent 后 set
        set({ cloudConsentSigned: true });
      },

      revokeCloudConsent: async () => {
        // T6.7 实现：调用 fileIpc.revokeCloudConsent 后 set
        set({ cloudConsentSigned: false });
      },

      clearError: () => set({ error: null }),
    }),
    {
      name: 'filemind-settings',
      // 仅持久化推断模式与同意书状态（快速启动显示；启动后 loadConfig 校正）
      partialize: (state) => ({
        inferenceMode: state.inferenceMode,
        cloudConsentSigned: state.cloudConsentSigned,
      }),
    },
  ),
);

/**
 * 将 Rust 端字符串推理模式归一化为 InferenceMode 枚举值。
 *
 * Rust AppConfig.inference_mode 是 string 类型（可能 'local' / 'cloud'），
 * 此处统一转 PascalCase 对齐 InferenceMode 枚举。
 */
function normalizeInferenceMode(raw: string): InferenceMode {
  const lower = raw.toLowerCase();
  if (lower === 'cloud') return 'Cloud';
  // 默认 Local（含未知值兜底，避免前端崩溃）
  return 'Local';
}

/** 根据推理模式返回状态栏显示的模型名。 */
function getLlmModelLabel(mode: InferenceMode): string {
  return mode === 'Cloud' ? 'OpenAI · gpt-4o-mini' : 'Ollama · Qwen2.5:7B';
}
