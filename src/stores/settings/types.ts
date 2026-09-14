// 设置 store 的公共类型与依赖面。
//
// 独立成文件的原因：动作工厂（config / cloud / ollama）与 store 本身都要引用
// SettingsState，放在 store 里会让子模块反向依赖 store（循环导入）。

import type {
  ApiKeyStatus,
  AppConfig,
  CloudProviderRecord,
  CloudProviderUpsertInput,
  EmbeddingModelAvailability,
  InferenceMode,
  OllamaModelInfo,
  OllamaStatus,
} from '@/types/ipc';
import type { ThemeMode } from '@/types/models';

/** 写入状态（zustand 的 set）：补丁与函数式更新两种形式都用得到。 */
export type SettingsSet = (
  partial: Partial<SettingsState> | ((state: SettingsState) => Partial<SettingsState>),
) => void;

/** 读当前状态（zustand 的 get）。 */
export type SettingsGet = () => SettingsState;

export interface SettingsState {
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
  /** 已签署时选择的云端提供商 slug（未签为 null） */
  cloudConsentProvider: string | null;
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
  /** 上次成功探测完成的时间戳（探测 TTL 节流用；瞬态不持久化） */
  lastOllamaProbeAt: number;
  /** 可选的 LLM 模型列表（来自 Ollama 探测） */
  llmModelOptions: OllamaModelInfo[];
  /** Embedding 模型可用性列表（来自 Ollama 探测） */
  embeddingModelOptions: EmbeddingModelAvailability[];
  /** 主题模式（跟随系统 / 亮色 / 暗色） */
  theme: ThemeMode;
  /** 各云服务商 API Key 状态（仅掩码提示，不含完整 Key；不持久化） */
  apiKeyStatus: Record<string, ApiKeyStatus>;
  /** 云端推理模型名（如 gpt-4o / deepseek-chat；空串表示未指定，走 Provider 默认） */
  cloudModel: string;
  /** 云端推理 Temperature（0-1，0.2 为通用默认） */
  temperature: number;
  /** 正在安装的 Embedding 模型名（null 表示无安装任务） */
  installingModel: string | null;
  /** 模型安装错误信息（null 表示无错误） */
  installError: string | null;
  /** P-07：用户自定义云提供商列表（来自 DB cloud_providers 表，设置页表单直接操作） */
  cloudProviders: CloudProviderRecord[];
  /** P-07：当前激活的云提供商 slug（app_config.active_cloud_provider），空串=未指定 */
  activeCloudProvider: string;
  /** P-07：提供商列表加载中（设置页卡片骨架屏用） */
  cloudProvidersLoading: boolean;

  /** 从 Rust 端加载完整配置（启动时调用） */
  loadConfig: () => Promise<void>;
  /** FE-M11：initFailed 后重试加载 */
  retryInit: () => Promise<void>;
  /** 切换推理模式（需用户主动操作，记录审计日志） */
  setInferenceMode: (mode: InferenceMode) => Promise<void>;
  /**
   * 探测本地 Ollama 环境（可用性 + 模型列表，设置页/引导页调用）。
   * 默认 60s 内复用上次成功结果（真实 HTTP 探测较慢，进页反复探测无意义）；
   * `force=true` 绕过节流（「重新检测」按钮 / 模型安装完成后）。
   */
  probeOllama: (force?: boolean) => Promise<void>;
  /** 切换本地 LLM 模型（乐观更新 + 持久化） */
  setLlmModel: (name: string) => Promise<void>;
  /** 更新配置（部分字段） */
  updateConfig: (partial: Partial<AppConfig>) => Promise<void>;
  /** 签署云端同意书 */
  signCloudConsent: (provider: string) => Promise<void>;
  /** 撤销云端同意书（自动切回 Local 模式） */
  revokeCloudConsent: () => Promise<void>;
  /** 加载各云服务商 API Key 状态（仅掩码提示，不含完整 Key） */
  loadApiKeyStatus: () => Promise<void>;
  /** 保存指定云服务商 API Key（存系统 Keychain；成功返回新状态） */
  setApiKey: (provider: string, key: string) => Promise<void>;
  /** 删除指定云服务商 API Key（Keychain） */
  deleteApiKey: (provider: string) => Promise<void>;
  /** 标记引导完成（写入 DB） */
  completeOnboarding: (dataDirectory: string) => Promise<void>;
  /** 切换主题模式（持久化 + 应用到 html data-theme） */
  setTheme: (mode: ThemeMode) => void;
  /** 清除错误 */
  clearError: () => void;
  /** 安装指定 Embedding 模型（从 Ollama 拉取） */
  installModel: (modelName: string) => Promise<void>;
  /** P-07：从 DB 拉取全部云提供商（覆盖 store） */
  loadCloudProviders: () => Promise<void>;
  /** P-07：新建或更新提供商（成功后自动刷新列表 + API Key 状态） */
  upsertCloudProvider: (input: CloudProviderUpsertInput) => Promise<CloudProviderRecord>;
  /** P-07：软删除指定 slug 的提供商（成功后自动刷新列表 + 若 slug 为当前激活则清空激活） */
  deleteCloudProvider: (providerKey: string) => Promise<void>;
  /** P-07：切换当前激活的云提供商 slug（空串清除；写入 DB 并更新 store） */
  setActiveCloudProvider: (providerKey: string) => Promise<void>;
}
