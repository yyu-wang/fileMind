// 云提供商管理区块的状态与动作：列表加载、新建/编辑、删除、激活四条流程。
//
// 从 CloudProviderManager 抽出——组件此前 250 行，其中大部分是这块 selector 与 handler，
// 与视觉结构无关。四条流程共用同一个区块级 error（与重构前语义一致：操作开始时先清空、
// 失败时写入），因此各自通过 errorSink 回写，而不是各持一份错误态。

import { useEffect, useState } from 'react';

import { useSettingsStore } from '@/stores/settingsStore';
import type { ApiKeyStatus, CloudProviderRecord, CloudProviderUpsertInput } from '@/types/ipc';

import type { CloudProviderRowActions, CloudProviderRowState } from './CloudProviderRowView';

/** 各子流程回写区块级错误文案的入口。 */
type CloudProviderErrorSink = (message: string | null) => void;

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

/** 新建 / 编辑表单：展开态、编辑对象与提交。 */
function useCloudProviderEditing(
  upsertCloudProvider: (input: CloudProviderUpsertInput) => Promise<CloudProviderRecord>,
  errorSink: CloudProviderErrorSink,
) {
  const [showCreate, setShowCreate] = useState(false);
  const [editing, setEditing] = useState<CloudProviderRecord | null>(null);
  const [submitting, setSubmitting] = useState(false);

  const startCreate = () => setShowCreate(true);
  const cancelCreate = () => setShowCreate(false);
  /** 进入某行的编辑态（同时收起新建表单，两者互斥） */
  const startEdit = (provider: CloudProviderRecord) => {
    setEditing(provider);
    setShowCreate(false);
  };
  const cancelEdit = () => setEditing(null);

  const submitUpsert = async (input: CloudProviderUpsertInput) => {
    setSubmitting(true);
    errorSink(null);
    try {
      await upsertCloudProvider(input);
      setShowCreate(false);
      setEditing(null);
    } catch (e) {
      errorSink(e instanceof Error ? e.message : '保存提供商失败');
    } finally {
      setSubmitting(false);
    }
  };

  return {
    showCreate,
    editing,
    submitting,
    startCreate,
    cancelCreate,
    startEdit,
    cancelEdit,
    submitUpsert,
  };
}

/** 删除提供商：二次确认对象与确认后执行。 */
function useCloudProviderRemoval(
  deleteCloudProvider: (providerKey: string) => Promise<void>,
  errorSink: CloudProviderErrorSink,
) {
  const [pendingDelete, setPendingDelete] = useState<CloudProviderRecord | null>(null);

  const requestDelete = (provider: CloudProviderRecord) => setPendingDelete(provider);
  const cancelDelete = () => setPendingDelete(null);

  const confirmDelete = async () => {
    if (!pendingDelete) return;
    const victim = pendingDelete;
    setPendingDelete(null);
    try {
      await deleteCloudProvider(victim.provider_key);
    } catch (e) {
      errorSink(e instanceof Error ? e.message : '删除提供商失败');
    }
  };

  return { pendingDelete, requestDelete, cancelDelete, confirmDelete };
}

/** 激活切换：写入 app_config.active_cloud_provider。 */
function useCloudProviderActivation(
  activeCloudProvider: string,
  setActiveCloudProvider: (providerKey: string) => Promise<void>,
  errorSink: CloudProviderErrorSink,
) {
  const [activating, setActivating] = useState<string | null>(null);

  const activate = async (providerKey: string) => {
    if (activeCloudProvider === providerKey) return;
    setActivating(providerKey);
    errorSink(null);
    try {
      await setActiveCloudProvider(providerKey);
    } catch (e) {
      errorSink(e instanceof Error ? e.message : '切换激活提供商失败');
    } finally {
      setActivating(null);
    }
  };

  return { activating, activate };
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
