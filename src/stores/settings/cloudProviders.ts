// P-07 自定义云提供商动作：列表加载、增删改与激活选择。
//
// 从 cloud.ts 拆出（该文件逼近 .ts 警告阈值 150）。与同意书 / API Key 两组动作的区别：
// 这一组围绕 cloud_providers 表本身的 CRUD，且每次写操作后都要连带刷新列表与 Key 状态。
//
// 由 createCloudActions 展开进 store，故 settingsStore 的装配方式不变。

import { fileIpc } from '@/lib/ipc';
import type { SettingsGet, SettingsSet, SettingsState } from './types';

/** 生成自定义云提供商动作（供 createCloudActions 展开）。 */
export function createCloudProviderActions(deps: {
  set: SettingsSet;
  get: SettingsGet;
}): Pick<
  SettingsState,
  'loadCloudProviders' | 'upsertCloudProvider' | 'deleteCloudProvider' | 'setActiveCloudProvider'
> {
  const { set, get } = deps;
  return {
    loadCloudProviders: async () => {
      set({ cloudProvidersLoading: true, error: null });
      try {
        const result = await fileIpc.listCloudProviders();
        if (result.status === 'ok') {
          set({ cloudProviders: result.data });
          // 提供商列表刷新后，一并刷新 API Key 状态（Key 状态按 slug 对齐）
          await get().loadApiKeyStatus();
        } else {
          set({ error: result.error });
        }
      } catch (e) {
        set({ error: e instanceof Error ? e.message : String(e) });
      } finally {
        set({ cloudProvidersLoading: false });
      }
    },

    upsertCloudProvider: async (input) => {
      const result = await fileIpc.upsertCloudProvider(input);
      if (result.status !== 'ok') {
        set({ error: result.error });
        throw new Error(result.error);
      }
      // 成功后刷新列表和 Key 状态（新 provider 立刻能在 Key 卡片看到）
      await get().loadCloudProviders();
      return result.data;
    },

    deleteCloudProvider: async (providerKey) => {
      const result = await fileIpc.deleteCloudProvider(providerKey);
      if (result.status !== 'ok') {
        set({ error: result.error });
        throw new Error(result.error);
      }
      // 删除 slug 正好是当前激活 → 清空激活（避免 DB 侧留着一个已删 slug 激活）
      if (get().activeCloudProvider === providerKey) {
        await get().setActiveCloudProvider('');
      }
      await get().loadCloudProviders();
    },

    setActiveCloudProvider: async (providerKey) => {
      await get().updateConfig({ active_cloud_provider: providerKey || null });
    },
  };
}
