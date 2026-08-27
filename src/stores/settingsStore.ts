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
import { CLOUD_CONSENT_VERSION } from '../lib/consent';
import { fileIpc } from '../lib/ipc';
import { applyTheme } from '../lib/theme';
import type {
  ApiKeyStatus,
  AppConfig,
  CloudProvider,
  EmbeddingModelAvailability,
  InferenceMode,
  OllamaModelInfo,
  OllamaStatus,
} from '../types/ipc';
import { ThemeMode } from '../types/models';

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
  /** 是否已完成首次启动引导 */
  onboardingCompleted: boolean;
  /** 云端同意书是否已签（云端模式前置条件） */
  cloudConsentSigned: boolean;
  /** 已签署的同意书版本号（未签为 null） */
  cloudConsentVersion: string | null;
  /** 已签署时选择的云端提供商（未签为 null） */
  cloudConsentProvider: CloudProvider | null;
  /** 签署时间（ISO 8601，未签为 null） */
  cloudConsentSignedAt: string | null;
  /** 配置加载中 */
  isLoading: boolean;
  /** FE-M11：启动配置加载失败（App 渲染错误卡片 + 重试入口） */
  initFailed: boolean;
  /** 错误信息（null 表示无错误） */
  error: string | null;
  /** Ollama 探测结果（null 表示未探测或探测失败） */
  ollamaStatus: OllamaStatus | null;
  /** Ollama 探测中 */
  ollamaProbing: boolean;
  /** 可选的 LLM 模型列表（来自 Ollama 探测） */
  llmModelOptions: OllamaModelInfo[];
  /** Embedding 模型可用性列表（来自 Ollama 探测） */
  embeddingModelOptions: EmbeddingModelAvailability[];
  /** 主题模式（跟随系统 / 亮色 / 暗色） */
  theme: ThemeMode;
  /** 各云服务商 API Key 状态（仅掩码提示，不含完整 Key；不持久化） */
  apiKeyStatus: Record<CloudProvider, ApiKeyStatus>;
  /** 云端推理模型名（如 gpt-4o / deepseek-chat；空串表示未指定，走 Provider 默认） */
  cloudModel: string;
  /** 云端推理 Temperature（0-1，0.2 为通用默认） */
  temperature: number;
  /** 正在安装的 Embedding 模型名（null 表示无安装任务） */
  installingModel: string | null;
  /** 模型安装错误信息（null 表示无错误） */
  installError: string | null;

  /** 从 Rust 端加载完整配置（启动时调用） */
  loadConfig: () => Promise<void>;
  /** FE-M11：initFailed 后重试加载 */
  retryInit: () => Promise<void>;
  /** 切换推理模式（需用户主动调用，记录审计日志） */
  setInferenceMode: (mode: InferenceMode) => Promise<void>;
  /** 探测本地 Ollama 环境（可用性 + 模型列表，设置页/引导页调用） */
  probeOllama: () => Promise<void>;
  /** 切换本地 LLM 模型（乐观更新 + 持久化） */
  setLlmModel: (name: string) => Promise<void>;
  /** 更新配置（部分字段） */
  updateConfig: (partial: Partial<AppConfig>) => Promise<void>;
  /** 签署云端同意书 */
  signCloudConsent: (provider: CloudProvider) => Promise<void>;
  /** 撤销云端同意书（自动切回 Local 模式） */
  revokeCloudConsent: () => Promise<void>;
  /** 加载各云服务商 API Key 状态（仅掩码提示，不含完整 Key） */
  loadApiKeyStatus: () => Promise<void>;
  /** 保存指定云服务商 API Key（存系统 Keychain；成功返回新状态） */
  setApiKey: (provider: CloudProvider, key: string) => Promise<void>;
  /** 删除指定云服务商 API Key（Keychain） */
  deleteApiKey: (provider: CloudProvider) => Promise<void>;
  /** 标记引导完成（写入 DB） */
  completeOnboarding: (dataDirectory: string) => Promise<void>;
  /** 切换主题模式（持久化 + 应用到 html data-theme） */
  setTheme: (mode: ThemeMode) => void;
  /** 清除错误 */
  clearError: () => void;
  /** 安装指定 Embedding 模型（从 Ollama 拉取） */
  installModel: (modelName: string) => Promise<void>;
}

/**
 * FE-M11：loadConfig 的实际加载逻辑（被 loadConfig 的 try/catch 包裹）。
 * 抽为独立函数：store action 内联时替换残留体困难，独立函数更清晰。
 * 业务错误（status==='error'）与 IPC 异常统一 throw，由调用方落 initFailed。
 */
async function loadConfigInner(set: (partial: Partial<SettingsState>) => void): Promise<void> {
  const result = await fileIpc.getConfig();
  if (result.status === 'ok') {
    const cfg = result.data;
    const mode = normalizeInferenceMode(cfg.inference_mode);
    set({
      dataDirectory: cfg.data_directory,
      inferenceMode: mode,
      embeddingModel: cfg.embedding_model,
      maxFileSizeMb: cfg.max_file_size_mb,
      language: cfg.language,
      onboardingCompleted: cfg.onboarding_completed,
      cloudConsentSigned: cfg.cloud_consent_signed,
      cloudConsentVersion: cfg.cloud_consent_version,
      cloudConsentProvider: cfg.cloud_consent_provider,
      cloudConsentSignedAt: cfg.cloud_consent_signed_at,
      llmModel: cfg.llm_model,
      cloudModel: cfg.cloud_model ?? '',
      isLoading: false,
      initFailed: false,
    });
  } else {
    throw new Error(result.error);
  }
}

export const useSettingsStore = create<SettingsState>()(
  persist(
    (set, get) => ({
      inferenceMode: 'Local',
      llmModel: 'qwen3.8-27b',
      dataDirectory: '',
      // FE-m12：对齐 Rust config.rs 默认值 bge-large-zh-v1.5
      embeddingModel: 'bge-large-zh-v1.5',
      maxFileSizeMb: 100,
      language: 'zh-CN',
      onboardingCompleted: false,
      cloudConsentSigned: false,
      cloudConsentVersion: null,
      cloudConsentProvider: null,
      cloudConsentSignedAt: null,
      isLoading: true,
      initFailed: false,
      error: null,
      ollamaStatus: null,
      ollamaProbing: false,
      llmModelOptions: [],
      embeddingModelOptions: [],
      theme: ThemeMode.System,
      apiKeyStatus: {
        Openai: { provider: 'Openai', has_key: false, hint: '' },
        Deepseek: { provider: 'Deepseek', has_key: false, hint: '' },
      },
      cloudModel: '',
      temperature: 0.2,
      installingModel: null,
      installError: null,

      loadConfig: async () => {
        set({ isLoading: true, error: null });
        // FE-M11：typedError 对 Error 实例直接 rethrow（specta 生成，不可改），
        // 必须兜底——否则 isLoading 永久 true，App 渲染「正在加载配置...」白屏
        try {
          await loadConfigInner(set);
        } catch (e) {
          set({
            isLoading: false,
            initFailed: true,
            error: e instanceof Error ? e.message : String(e),
          });
        }
      },

      retryInit: async () => {
        await get().loadConfig();
      },

      setInferenceMode: async (mode) => {
        // source='ui' 标识来自前端用户主动操作（04 API §2-3b 安全约束）
        const result = await fileIpc.setInferenceMode(mode, 'ui');
        if (result.status === 'ok') {
          // 模式切换不改变用户已选的 LLM 模型（llmModel 保持真实模型名）
          set({ inferenceMode: mode });
        } else {
          set({ error: result.error });
          throw new Error(result.error);
        }
      },

      probeOllama: async () => {
        set({ ollamaProbing: true, error: null });
        try {
          const result = await fileIpc.ollamaStatus();
          if (result.status === 'ok') {
            set({
              ollamaStatus: result.data,
              llmModelOptions: result.data.llm_models,
              embeddingModelOptions: result.data.embedding_models,
            });
          } else {
            set({ ollamaStatus: null, error: result.error });
          }
        } finally {
          set({ ollamaProbing: false });
        }
      },

      setLlmModel: async (name) => {
        // FE-M5：乐观更新 + 失败回滚（此前失败不回滚，下拉显示未持久化的模型）
        const prev = get().llmModel;
        set({ llmModel: name });
        try {
          await get().updateConfig({ llm_model: name });
        } catch (e) {
          // 仅当 store 值仍是本次乐观值时回滚：updateConfig 成功后的 loadConfig
          // 重拉若再抛错（网络抖动），值其实已持久化，回滚是误伤
          if (get().llmModel === name) {
            set({ llmModel: prev, error: e instanceof Error ? e.message : String(e) });
          }
          throw e;
        }
      },

      updateConfig: async (partial) => {
        // Rust 端要求完整 AppConfig，先合并当前值再发送
        const current = get();
        const fullConfig: AppConfig = {
          data_directory: partial.data_directory ?? current.dataDirectory,
          inference_mode: partial.inference_mode ?? current.inferenceMode.toLowerCase(),
          embedding_model: partial.embedding_model ?? current.embeddingModel,
          llm_model: partial.llm_model ?? current.llmModel,
          max_file_size_mb: partial.max_file_size_mb ?? current.maxFileSizeMb,
          language: partial.language ?? current.language,
          onboarding_completed: partial.onboarding_completed ?? current.onboardingCompleted,
          cloud_consent_signed: partial.cloud_consent_signed ?? current.cloudConsentSigned,
          // 兜底必须回填当前值而非 null：后端 upsert 是全字段覆盖，
          // 置空会静默清掉已签署的同意书记录（合规审计链断裂）
          cloud_consent_version: partial.cloud_consent_version ?? current.cloudConsentVersion,
          cloud_consent_provider: partial.cloud_consent_provider ?? current.cloudConsentProvider,
          cloud_consent_signed_at: partial.cloud_consent_signed_at ?? current.cloudConsentSignedAt,
          cloud_model: partial.cloud_model ?? current.cloudModel,
        };
        const result = await fileIpc.updateConfig(fullConfig);
        if (result.status !== 'ok') {
          set({ error: result.error });
          throw new Error(result.error);
        }
        // 成功后重新加载完整配置，保证状态一致
        await get().loadConfig();
      },

      signCloudConsent: async (provider) => {
        // 同意书版本与后端 signCloudConsent 的 consent_version 一致（共享常量）
        const result = await fileIpc.signCloudConsent(CLOUD_CONSENT_VERSION, provider);
        if (result.status === 'ok') {
          set({
            cloudConsentSigned: true,
            inferenceMode: 'Cloud',
            cloudConsentVersion: CLOUD_CONSENT_VERSION,
            cloudConsentProvider: provider,
            cloudConsentSignedAt: new Date().toISOString(),
          });
        } else {
          set({ error: result.error });
          throw new Error(result.error);
        }
      },

      revokeCloudConsent: async () => {
        const result = await fileIpc.revokeCloudConsent();
        if (result.status === 'ok') {
          // 撤回后自动切回 Local（04 API §2-3d 联动，Rust 端已完成 DB 切换），
          // 同意元数据一并清空
          set({
            cloudConsentSigned: false,
            inferenceMode: 'Local',
            cloudConsentVersion: null,
            cloudConsentProvider: null,
            cloudConsentSignedAt: null,
          });
        } else {
          set({ error: result.error });
          throw new Error(result.error);
        }
      },

      loadApiKeyStatus: async () => {
        const result = await fileIpc.getApiKeyStatus();
        if (result.status === 'ok') {
          // FE-M12：合并构建而非整体替换——后端只返回部分 provider 时，
          // 未返回的项保留旧状态并补默认值，避免组件读到 undefined 崩溃
          const prev = get().apiKeyStatus;
          const map = {} as Record<CloudProvider, ApiKeyStatus>;
          for (const provider of Object.keys(prev) as CloudProvider[]) {
            map[provider] = { provider, has_key: false, hint: '' };
          }
          for (const status of result.data) map[status.provider] = status;
          set({ apiKeyStatus: map });
        } else {
          set({ error: result.error });
        }
      },

      setApiKey: async (provider, key) => {
        const result = await fileIpc.setApiKey(provider, key);
        if (result.status === 'ok') {
          // 只更新该服务商状态；Rust 侧返回的只有掩码 hint，不回传完整 Key
          set((state) => ({ apiKeyStatus: { ...state.apiKeyStatus, [provider]: result.data } }));
        } else {
          set({ error: result.error });
          throw new Error(result.error);
        }
      },

      deleteApiKey: async (provider) => {
        const result = await fileIpc.deleteApiKey(provider);
        if (result.status === 'ok') {
          set((state) => ({ apiKeyStatus: { ...state.apiKeyStatus, [provider]: result.data } }));
        } else {
          set({ error: result.error });
          throw new Error(result.error);
        }
      },

      completeOnboarding: async (dataDirectory) => {
        // 引导完成：更新 data_directory + onboarding_completed=true
        await get().updateConfig({ data_directory: dataDirectory, onboarding_completed: true });
      },

      setTheme: (mode) => {
        set({ theme: mode });
        applyTheme(mode);
      },

      installModel: async (modelName) => {
        set({ installingModel: modelName, installError: null });
        try {
          const result = await fileIpc.installEmbeddingModel(modelName);
          if (result.status === 'ok') {
            if (result.data.success) {
              await get().probeOllama();
            } else {
              set({ installError: result.data.message });
            }
          } else {
            set({ installError: result.error });
          }
        } catch (e) {
          set({ installError: e instanceof Error ? e.message : String(e) });
        } finally {
          set({ installingModel: null });
        }
      },

      clearError: () => set({ error: null }),
    }),
    {
      name: 'filemind-settings',
      // 仅持久化推断模式与同意书状态（快速启动显示；启动后 loadConfig 校正）
      partialize: (state) => ({
        inferenceMode: state.inferenceMode,
        cloudConsentSigned: state.cloudConsentSigned,
        onboardingCompleted: state.onboardingCompleted,
        theme: state.theme,
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
