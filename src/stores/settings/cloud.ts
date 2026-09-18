// 云端相关动作：同意书签署/撤销、API Key 状态（P-07 提供商 CRUD 见 ./cloudProviders.ts）。
//
// 与 store 分离的原因：这一组动作都围绕「云端可用性」这一条主线（同意书是前置条件，
// API Key 与提供商列表互相刷新），且共用一个错误处理形态（失败写 error + throw）。
//
// 拆分（原单文件 165 行，逼近 .ts 警告阈值 150）：P-07 的 4 个提供商 CRUD 动作移到
// ./cloudProviders.ts，由本工厂展开——settingsStore 的装配方式（只调 createCloudActions）
// 保持不变。

import { fileIpc } from '@/lib/ipc';
import { CLOUD_CONSENT_VERSION } from '@/lib/consent';
import type { ApiKeyStatus } from '@/types/ipc';
import { createCloudProviderActions } from './cloudProviders';
import type { SettingsGet, SettingsSet, SettingsState } from './types';

/**
 * 生成云端相关动作（供 store 展开进 create 的返回对象）。
 *
 * Args:
 *   deps: store 注入的 set / get
 *
 * Returns:
 *   同意书 / API Key / 云提供商三组动作
 */
export function createCloudActions(deps: {
  set: SettingsSet;
  get: SettingsGet;
}): Pick<
  SettingsState,
  | 'signCloudConsent'
  | 'revokeCloudConsent'
  | 'loadApiKeyStatus'
  | 'setApiKey'
  | 'deleteApiKey'
  | 'loadCloudProviders'
  | 'upsertCloudProvider'
  | 'deleteCloudProvider'
  | 'setActiveCloudProvider'
> {
  const { set, get } = deps;
  return {
    signCloudConsent: async (provider) => {
      // 同意书版本与后端 signCloudConsent 的 consent_version 一致（共享常量）
      const result = await fileIpc.signCloudConsent(CLOUD_CONSENT_VERSION, provider);
      if (result.status === 'ok') {
        // 后端 SIGN_CONSENT_SQL 已同步写 active_cloud_provider = ?2，
        // 前端这里同步状态确保 UI 立刻显示激活状态
        set({
          cloudConsentSigned: true,
          inferenceMode: 'Cloud',
          cloudConsentVersion: CLOUD_CONSENT_VERSION,
          cloudConsentProvider: provider,
          cloudConsentSignedAt: new Date().toISOString(),
          activeCloudProvider: provider,
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
        // 同意元数据与 activeCloudProvider 一并清空
        set({
          cloudConsentSigned: false,
          inferenceMode: 'Local',
          cloudConsentVersion: null,
          cloudConsentProvider: null,
          cloudConsentSignedAt: null,
          activeCloudProvider: '',
        });
      } else {
        set({ error: result.error });
        throw new Error(result.error);
      }
    },

    loadApiKeyStatus: async () => {
      const result = await fileIpc.getApiKeyStatus();
      if (result.status === 'ok') {
        // FE-M12：以当前 cloudProviders 的 slug 为基准建默认值，
        // 合并后端返回的真实 Key 状态；后端未返回=无 Key。
        const map = {} as Record<string, ApiKeyStatus>;
        for (const p of get().cloudProviders) {
          map[p.provider_key] = {
            provider: p.provider_key,
            has_key: false,
            hint: '',
          };
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

    // ---------- P-07：用户自定义云提供商 CRUD + 激活选择（见 ./cloudProviders.ts） ----------
    ...createCloudProviderActions(deps),
  };
}
