// 云提供商管理区块的状态与动作：列表加载 + 编排新建/编辑、删除、激活三条子流程。
//
// 从 CloudProviderManager 抽出——组件此前 250 行，其中大部分是这块 selector 与 handler，
// 与视觉结构无关。三条子流程共用同一个区块级 error（与重构前语义一致：操作开始时先清空、
// 失败时写入），因此各自通过 errorSink 回写，而不是各持一份错误态。
//
// 子流程 hook 本身已拆到同目录 useCloudProviderFlows.ts
//（原单文件 218 行，逼近 .ts 警告阈值 200）。

import { useEffect, useState } from 'react';

import { useSettingsStore } from '@/stores/settingsStore';
import type { ApiKeyStatus, CloudProviderRecord, CloudProviderUpsertInput } from '@/types/ipc';

import type { CloudProviderRowActions, CloudProviderRowState } from './CloudProviderRowView';
import {
  useCloudProviderActivation,
  useCloudProviderEditing,
  useCloudProviderRemoval,
} from './useCloudProviderFlows';

/** useCloudProviderManager 的对外出口。 */
export interface CloudProviderManagerHandle {
  /** 提供商列表（来自 store） */
  providers: CloudProviderRecord[];
  /** 列表加载中（骨架屏用） */
  loading: boolean;
  /** 各提供商 Key 状态（行内展示用） */
  apiKeyStatus: Record<string, ApiKeyStatus>;
  /** 列表行的状态 */
  rowState: CloudProviderRowState;
  /** 列表行的动作 */
  rowActions: CloudProviderRowActions;
  /** 是否展开新建表单 */
  showCreate: boolean;
  /** 正在编辑的提供商（null 表示无） */
  editing: CloudProviderRecord | null;
  /** 表单提交中 */
  submitting: boolean;
  /** 展开新建表单 */
  startCreate: () => void;
  /** 收起新建表单 */
  cancelCreate: () => void;
  /** 新建或更新提供商（校验通过后由表单触发） */
  submitUpsert: (input: CloudProviderUpsertInput) => Promise<void>;
  /** 区块级错误文案（null 表示无） */
  error: string | null;
  /** 待二次确认删除的提供商（null 表示无） */
  pendingDelete: CloudProviderRecord | null;
  /** 取消删除 */
  cancelDelete: () => void;
  /** 确认删除 */
  confirmDelete: () => Promise<void>;
}

/** 列表数据与「挂载即加载」（loadCloudProviders 内部会连带刷新 Key 状态）。 */
function useCloudProviderData() {
  const cloudProviders = useSettingsStore((s) => s.cloudProviders);
  const loading = useSettingsStore((s) => s.cloudProvidersLoading);
  const activeCloudProvider = useSettingsStore((s) => s.activeCloudProvider);
  const apiKeyStatus = useSettingsStore((s) => s.apiKeyStatus);
  const upsertCloudProvider = useSettingsStore((s) => s.upsertCloudProvider);
  const deleteCloudProvider = useSettingsStore((s) => s.deleteCloudProvider);
  const setActiveCloudProvider = useSettingsStore((s) => s.setActiveCloudProvider);
  const loadCloudProviders = useSettingsStore((s) => s.loadCloudProviders);

  useEffect(() => {
    void loadCloudProviders();
  }, [loadCloudProviders]);

  return {
    cloudProviders,
    loading,
    activeCloudProvider,
    apiKeyStatus,
    upsertCloudProvider,
    deleteCloudProvider,
    setActiveCloudProvider,
  };
}

/**
 * 管理「云提供商管理」区块的全部状态与动作。
 *
 * Returns:
 *   列表数据、行状态/动作、表单与删除确认的开关（见 CloudProviderManagerHandle）
 */
export function useCloudProviderManager(): CloudProviderManagerHandle {
  const data = useCloudProviderData();
  const [error, setError] = useState<string | null>(null);

  const form = useCloudProviderEditing(data.upsertCloudProvider, setError);
  const removal = useCloudProviderRemoval(data.deleteCloudProvider, setError);
  const activation = useCloudProviderActivation(
    data.activeCloudProvider,
    data.setActiveCloudProvider,
    setError,
  );

  const rowState: CloudProviderRowState = {
    activeProviderKey: data.activeCloudProvider,
    activating: activation.activating,
    editing: form.editing,
    submitting: form.submitting,
  };
  const rowActions: CloudProviderRowActions = {
    onActivate: (providerKey) => void activation.activate(providerKey),
    onEdit: form.startEdit,
    onCancelEdit: form.cancelEdit,
    onRequestDelete: removal.requestDelete,
    onSubmit: form.submitUpsert,
  };

  return {
    providers: data.cloudProviders,
    loading: data.loading,
    apiKeyStatus: data.apiKeyStatus,
    rowState,
    rowActions,
    showCreate: form.showCreate,
    editing: form.editing,
    submitting: form.submitting,
    startCreate: form.startCreate,
    cancelCreate: form.cancelCreate,
    submitUpsert: form.submitUpsert,
    error,
    pendingDelete: removal.pendingDelete,
    cancelDelete: removal.cancelDelete,
    confirmDelete: removal.confirmDelete,
  };
}
