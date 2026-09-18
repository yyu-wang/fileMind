// Key 行列表：按 store 中的 Provider 顺序渲染 CloudApiKeyRow。
//
// 从 CloudApiKeySection 抽出——原为容器里一段 map（每行要拼装草稿、在途状态与三个回调）。

import type { ApiKeyStatus, CloudProviderRecord } from '@/types/ipc';

import { CloudApiKeyRow } from './CloudApiKeyRow';

interface CloudApiKeyListProps {
  /** Provider 列表 */
  providers: CloudProviderRecord[];
  /** 各 Provider 的 Key 状态 */
  apiKeyStatus: Record<string, ApiKeyStatus>;
  /** 各 Provider 输入框的草稿值 */
  drafts: Record<string, string>;
  /** 有请求在途的 Provider（null 表示空闲） */
  busyProvider: string | null;
  /** 更新某 Provider 的输入框草稿 */
  onDraftChange: (providerKey: string, value: string) => void;
  /** 保存某 Provider 的 Key */
  onSave: (providerKey: string) => void;
  /** 请求删除某 Provider 的 Key（先弹二次确认） */
  onRequestRemove: (providerKey: string) => void;
}

export function CloudApiKeyList({
  providers,
  apiKeyStatus,
  drafts,
  busyProvider,
  onDraftChange,
  onSave,
  onRequestRemove,
}: CloudApiKeyListProps) {
  return (
    <>
      {providers.map((provider) => (
        <CloudApiKeyRow
          key={provider.provider_key}
          provider={provider}
          status={apiKeyStatus[provider.provider_key]}
          row={{
            draft: drafts[provider.provider_key] ?? '',
            busy: busyProvider === provider.provider_key,
          }}
          actions={{
            onChangeDraft: (value) => onDraftChange(provider.provider_key, value),
            onSave: () => onSave(provider.provider_key),
            onRequestRemove: () => onRequestRemove(provider.provider_key),
          }}
        />
      ))}
    </>
  );
}
