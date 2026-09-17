// SettingsState 的数据字段（从 types.ts 拆出：该文件逼近 .ts 警告阈值 150）。
//
// 拆分线是「数据字段」与「动作签名」，由 SettingsState 组合（`interface SettingsState
// extends SettingsDataState`）——对外仍只有 SettingsState 一个名字，各子模块与 store
// 的引用方式不变。

import type {
  ApiKeyStatus,
  CloudProviderRecord,
  EmbeddingModelAvailability,
  InferenceMode,
  ModelDownloadStatus,
  ModelImportResult,
  OllamaModelInfo,
  OllamaStatus,
} from '@/types/ipc';
import type { ThemeMode } from '@/types/models';

export interface SettingsDataState {
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
  /** Embedding 模型状态列表（来自探测；`available` 表示本地模型文件是否就绪） */
  embeddingModelOptions: EmbeddingModelAvailability[];
  /** 主题模式（跟随系统 / 亮色 / 暗色） */
  theme: ThemeMode;
  /** 各云服务商 API Key 状态（仅掩码提示，不含完整 Key；不持久化） */
  apiKeyStatus: Record<string, ApiKeyStatus>;
  /** 云端推理模型名（如 gpt-4o / deepseek-chat；空串表示未指定，走 Provider 默认） */
  cloudModel: string;
  /** 本地生成后端：'ollama'（默认）或 'builtin'（Sidecar 内置 llama.cpp 引擎） */
  localLlmBackend: string;
  /** 内置后端的 GGUF 模型标识（模型目录名，非文件名） */
  localLlmModel: string;
  /** 云端推理 Temperature（0-1，0.2 为通用默认） */
  temperature: number;
  /** 正在下载的 Embedding 模型名（null 表示无下载任务） */
  downloadingModel: string | null;
  /**
   * 各模型最近一次下载状态（按 model_name 索引；未查询过的模型不在表内）。
   *
   * 用映射而不是单个槽位：设置页同时挂载 Embedding / Rerank / 本地 GGUF 三张模型卡片，
   * 单槽会被最后一次写入的模型独占——另一张卡片便读不到自己的状态（例如 Rerank 已就绪
   * 却显示「未下载」）。
   */
  modelDownloads: Record<string, ModelDownloadStatus>;
  /**
   * 各模型「启动下载请求失败」的原因（按 model_name 索引）。
   *
   * 与 modelDownloads 同理按模型名分槽：共享单槽会让 A 模型卡片显示 B 模型的失败原因，
   * 或让未参与下载的卡片永远看不到属于自己的失败（静默失败）。
   */
  installErrors: Record<string, string>;
  /** 离线模型包导入进行中（本地拷贝数百 MB~数 GB，耗时长，用于禁用入口与切换文案） */
  importingPackage: boolean;
  /** 最近一次离线模型包导入结果（null 表示尚未导入） */
  importResult: ModelImportResult | null;
  /**
   * 离线导入错误（含 EMB-V-001 / EMB-U-002 错误码与缺失文件明细）。
   *
   * 不复用通用的 `error`：那个字段由 Ollama 探测写入、且每次探测开始即被清空，
   * 会让「导入失败」被一次后台探测擦掉，或在未导入时误显示探测错误。
   */
  importError: string | null;
  /** P-07：用户自定义云提供商列表（来自 DB cloud_providers 表，设置页表单直接操作） */
  cloudProviders: CloudProviderRecord[];
  /** P-07：当前激活的云提供商 slug（app_config.active_cloud_provider），空串=未指定 */
  activeCloudProvider: string;
  /** P-07：提供商列表加载中（设置页卡片骨架屏用） */
  cloudProvidersLoading: boolean;
}
