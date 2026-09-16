// 云端 API Key 区块的状态与动作：Provider 行加载、Key 草稿与增删、云端模型草稿。
//
// 从 CloudApiKeySection 抽出——组件此前 244 行，其中大部分是这块 selector 与 handler，
// 与视觉结构无关。三个流程（模型保存 / Key 保存 / Key 删除）共用同一个区块级 error
// （与重构前语义一致：操作开始时先清空、失败时写入），因此各自通过 errorSink 回写。

import { useEffect, useState } from 'react';

import { useSettingsStore } from '@/stores/settingsStore';
import type { ApiKeyStatus, CloudProviderRecord } from '@/types/ipc';

/** 各子流程回写区块级错误文案的入口。 */
type ApiKeyErrorSink = (message: string | null) => void;

/** 统一错误文案：Error 用 message，其余用兜底文案。 */
function messageOf(err: unknown, fallback: string): string {
  return err instanceof Error ? err.message : fallback;
}

/** useCloudApiKeySection 的对外出口。 */
export interface CloudApiKeySectionHandle {
  /** Provider 列表（Key 行的基准） */
  providers: CloudProviderRecord[];
  /** 各 Provider 的 Key 状态 */
  apiKeyStatus: Record<string, ApiKeyStatus>;
  /** 各 Provider 输入框的草稿值 */
  drafts: Record<string, string>;
  /** 有请求在途的 Provider（null 表示空闲） */
  busyProvider: string | null;
  /** 区块级错误文案（null 表示无） */
  error: string | null;
  /** 待二次确认删除的 Provider slug（null 表示无） */
  pendingRemove: string | null;
  /** 待删除 Provider 的显示名（找不到时回退为 slug） */
  pendingRemoveName: string;
  /** 打开/关闭删除确认（传 slug 打开，传 null 关闭） */
  setPendingRemove: (providerKey: string | null) => void;
  /** 更新某 Provider 的输入框草稿 */
  changeDraft: (providerKey: string, value: string) => void;
  /** 保存某 Provider 的 API Key（空输入直接提示，不发请求） */
  save: (providerKey: string) => Promise<void>;
  /** 确认删除（先关闭确认框再执行） */
  confirmRemove: () => Promise<void>;
  /** 云端模型输入框当前值（至少回显 store 已有值） */
  displayModelDraft: string;
  /** 更新云端模型草稿 */
  setModelDraft: (value: string) => void;
  /** 模型保存中 */
  savingModel: boolean;
  /** 保存云端模型名（空值直接提示） */
  saveModel: () => Promise<void>;
}

/** Provider 列表加载：挂载时补全一次（列表为空先拉列表，否则刷新 Key 状态）。 */
function useProviderListLoad(
  providers: CloudProviderRecord[],
  loadApiKeyStatus: () => Promise<void>,
  loadCloudProviders: () => Promise<void>,
): void {
  useEffect(() => {
    void (async () => {
      // Provider 列表是 Key 行的基准；先 loadCloudProviders（其内部会连带
      // 调 loadApiKeyStatus），保证界面至少渲染内置的 2 条。
      if (providers.length === 0) {
        await loadCloudProviders();
      } else {
        await loadApiKeyStatus();
      }
    })();
  }, [loadCloudProviders, loadApiKeyStatus, providers.length]);
}

/** 云端模型名：草稿、保存中与保存。 */
function useCloudModelSetting(errorSink: ApiKeyErrorSink) {
  const cloudModel = useSettingsStore((s) => s.cloudModel);
  const updateConfig = useSettingsStore((s) => s.updateConfig);
  const [modelDraft, setModelDraft] = useState(() => cloudModel || '');
  const [savingModel, setSavingModel] = useState(false);

  // Store 更新时，如果当前输入框仍为空则回填一次；不使用 setState-in-effect，
  // 而是把赋值推到 setTimeout(0) 延后执行，避免级联重渲染。
  if (cloudModel && !modelDraft) {
    window.setTimeout(() => setModelDraft(cloudModel), 0);
  }
  // 取派生显示值，保证至少展示 store 已有值
  const displayModelDraft = modelDraft || cloudModel || '';

  const saveModel = async () => {
    const trimmedModel = displayModelDraft.trim();
    if (!trimmedModel) {
      errorSink('云端模型名不能为空');
      return;
    }
    setSavingModel(true);
    errorSink(null);
    try {
      await updateConfig({ cloud_model: trimmedModel });
    } catch (e) {
      errorSink(messageOf(e, '保存云端模型配置失败'));
    } finally {
      setSavingModel(false);
    }
  };

  return { displayModelDraft, setModelDraft, savingModel, saveModel };
}

/** 各 Provider 的 Key 草稿、保存与删除。 */
function useApiKeyRows(errorSink: ApiKeyErrorSink) {
  const setApiKey = useSettingsStore((s) => s.setApiKey);
  const deleteApiKey = useSettingsStore((s) => s.deleteApiKey);
  const [drafts, setDrafts] = useState<Record<string, string>>({});
  const [busyProvider, setBusyProvider] = useState<string | null>(null);
  const [pendingRemove, setPendingRemove] = useState<string | null>(null);

  const changeDraft = (providerKey: string, value: string) => {
    setDrafts((d) => ({ ...d, [providerKey]: value }));
  };

  const save = async (providerKey: string) => {
    const key = (drafts[providerKey] ?? '').trim();
    if (!key) {
      errorSink('请输入 API Key 后再保存');
      return;
    }
    setBusyProvider(providerKey);
    errorSink(null);
    try {
      await setApiKey(providerKey, key);
      setDrafts((d) => ({ ...d, [providerKey]: '' }));
    } catch (e) {
      errorSink(messageOf(e, '保存 API Key 失败'));
    } finally {
      setBusyProvider(null);
    }
  };

  const remove = async (providerKey: string) => {
    setBusyProvider(providerKey);
    errorSink(null);
    try {
      await deleteApiKey(providerKey);
    } catch (e) {
      errorSink(messageOf(e, '删除 API Key 失败'));
    } finally {
      setBusyProvider(null);
    }
  };

  return { drafts, busyProvider, pendingRemove, setPendingRemove, changeDraft, save, remove };
}

/**
 * 管理「AI 模型配置」区块的全部状态与动作。
 *
 * Returns:
 *   Provider 行数据、Key 草稿与增删、模型草稿与保存（见 CloudApiKeySectionHandle）
 */
export function useCloudApiKeySection(): CloudApiKeySectionHandle {
  const apiKeyStatus = useSettingsStore((s) => s.apiKeyStatus);
  const providers = useSettingsStore((s) => s.cloudProviders);
  const loadApiKeyStatus = useSettingsStore((s) => s.loadApiKeyStatus);
  const loadCloudProviders = useSettingsStore((s) => s.loadCloudProviders);
  const [error, setError] = useState<string | null>(null);

  useProviderListLoad(providers, loadApiKeyStatus, loadCloudProviders);
  const model = useCloudModelSetting(setError);
  const keys = useApiKeyRows(setError);
  const pendingRemoveName =
    providers.find((p) => p.provider_key === keys.pendingRemove)?.name ?? keys.pendingRemove ?? '';

  /** 确认删除：先关闭确认框再执行（与重构前一致） */
  const confirmRemove = async () => {
    if (keys.pendingRemove === null) return;
    const providerKey = keys.pendingRemove;
    keys.setPendingRemove(null);
    await keys.remove(providerKey);
  };

  return {
    providers,
    apiKeyStatus,
    drafts: keys.drafts,
    busyProvider: keys.busyProvider,
    error,
    pendingRemove: keys.pendingRemove,
    pendingRemoveName,
    setPendingRemove: keys.setPendingRemove,
    changeDraft: keys.changeDraft,
    save: keys.save,
    confirmRemove,
    displayModelDraft: model.displayModelDraft,
    setModelDraft: model.setModelDraft,
    savingModel: model.savingModel,
    saveModel: model.saveModel,
  };
}
