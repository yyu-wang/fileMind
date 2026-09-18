// 设置 store 的公共类型与依赖面。
//
// 独立成文件的原因：动作工厂（config / cloud / ollama）与 store 本身都要引用
// SettingsState，放在 store 里会让子模块反向依赖 store（循环导入）。
//
// 拆分（原单文件 163 行，逼近 .ts 警告阈值 150）：数据字段移到 ./stateFields.ts，
// 本文件保留依赖面（set/get）与动作签名；SettingsState 组合两者，对外仍是同一个名字。

import type {
  AppConfig,
  CloudProviderRecord,
  CloudProviderUpsertInput,
  InferenceMode,
} from '@/types/ipc';
import type { ThemeMode } from '@/types/models';
import type { SettingsDataState } from './stateFields';

/** 写入状态（zustand 的 set）：补丁与函数式更新两种形式都用得到。 */
export type SettingsSet = (
  partial: Partial<SettingsState> | ((state: SettingsState) => Partial<SettingsState>),
) => void;

/** 读当前状态（zustand 的 get）。 */
export type SettingsGet = () => SettingsState;

export interface SettingsState extends SettingsDataState {
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
  /** 启动（或手动重试）Embedding 模型下载：模型由 Sidecar 从 HF 镜像下载 */
  startModelDownload: (modelName: string) => Promise<void>;
  /** 查询 Embedding 模型下载状态（设置页轮询用；失败只记录错误，不抛） */
  refreshModelDownload: (modelName: string) => Promise<void>;
  /**
   * 导入离线模型包（zip 文件，或 `models` 目录 / 单个模型目录）。
   * 内网 / 无外网部署时用另一台已下载好模型的机器分发模型文件，避免检索时缺模型。
   */
  importModelPackage: (path: string) => Promise<void>;
  /** P-07：从 DB 拉取全部云提供商（覆盖 store） */
  loadCloudProviders: () => Promise<void>;
  /** P-07：新建或更新提供商（成功后自动刷新列表 + API Key 状态） */
  upsertCloudProvider: (input: CloudProviderUpsertInput) => Promise<CloudProviderRecord>;
  /** P-07：软删除指定 slug 的提供商（成功后自动刷新列表 + 若 slug 为当前激活则清空激活） */
  deleteCloudProvider: (providerKey: string) => Promise<void>;
  /** P-07：切换当前激活的云提供商 slug（空串清除；写入 DB 并更新 store） */
  setActiveCloudProvider: (providerKey: string) => Promise<void>;
}
