// 全局设置 store：推理模式、模型名、数据目录、云端同意书状态。
//
// 持久化策略：inferenceMode + cloudConsentSigned 走 localStorage（快速启动显示），
// 启动后 main.tsx 调 loadConfig() 从 Rust 端校正真值
//
// 设计原则（03 设计稿 1.1 隐私可见）：
// - 推理模式切换需用户主动操作，不静默
// - MODE_SWITCH_FORBIDDEN 错误码是安全阀门（04 API §2-3）
//
// 本文件只保留「状态 + 装配」（complexity 规则：.ts 强制上限 250 行），动作按职责
// 落到 ./settings/ 子模块；对外导入路径 @/stores/settingsStore 与公开符号保持不变。
//   ./settings/types.ts    状态结构 + 动作工厂的依赖面
//   ./settings/config.ts   加载配置 / 推理模式 / 更新配置 / 本地模型 / 完成引导
//   ./settings/cloud.ts    同意书 / API Key / 云提供商 CRUD 与激活
//   ./settings/ollama.ts   本地 Ollama 探测与 Embedding 模型安装

import { create } from 'zustand';
import { persist } from 'zustand/middleware';
import { applyTheme } from '../lib/theme';
import { ThemeMode } from '../types/models';
import { createCloudActions } from './settings/cloud';
import { createConfigActions } from './settings/config';
import { createOllamaActions } from './settings/ollama';
import type { SettingsState } from './settings/types';

export type { SettingsState } from './settings/types';

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
      lastOllamaProbeAt: 0,
      llmModelOptions: [],
      embeddingModelOptions: [],
      theme: ThemeMode.System,
      apiKeyStatus: {},
      cloudModel: '',
      temperature: 0.2,
      installingModel: null,
      installError: null,
      cloudProviders: [],
      activeCloudProvider: '',
      cloudProvidersLoading: false,

      ...createConfigActions({ set, get }),
      ...createCloudActions({ set, get }),
      ...createOllamaActions({ set, get }),

      setTheme: (mode) => {
        set({ theme: mode });
        applyTheme(mode);
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
