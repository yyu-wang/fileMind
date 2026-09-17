// 云提供商操作的三条子流程 hook：新建/编辑、删除、激活切换。
//
// 从 useCloudProviderManager 拆出（该文件逼近 .ts 警告阈值 200）。三者共用区块级
// error（操作开始时先清空、失败时写入），故各自通过 errorSink 回写，而不是各持一份
// 错误态。

import { useState } from 'react';

import type { CloudProviderRecord, CloudProviderUpsertInput } from '@/types/ipc';

/** 各子流程回写区块级错误文案的入口。 */
export type CloudProviderErrorSink = (message: string | null) => void;

/** 新建 / 编辑表单：展开态、编辑对象与提交。 */
export function useCloudProviderEditing(
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
export function useCloudProviderRemoval(
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
export function useCloudProviderActivation(
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
