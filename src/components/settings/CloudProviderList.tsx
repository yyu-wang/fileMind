// 提供商列表：<ul> + 每行的「编辑表单 ↔ 展示态」切换。
//
// 从 CloudProviderManager 抽出——原为一行 130 行的 map 回调（含行内联编辑分支）。

import type { ApiKeyStatus, CloudProviderRecord } from '@/types/ipc';

import { CloudProviderFormCard } from './CloudProviderFormCard';
import {
  CloudProviderRowView,
  type CloudProviderRowActions,
  type CloudProviderRowState,
} from './CloudProviderRowView';

interface CloudProviderListProps {
  /** 提供商列表 */
  providers: CloudProviderRecord[];
  /** 各提供商 Key 状态（行内展示用） */
  apiKeyStatus: Record<string, ApiKeyStatus>;
  /** 行状态 */
  state: CloudProviderRowState;
  /** 行动作 */
  actions: CloudProviderRowActions;
}

export function CloudProviderList({
  providers,
  apiKeyStatus,
  state,
  actions,
}: CloudProviderListProps) {
  return (
    <ul className="provider-list" style={{ listStyle: 'none', padding: 0, marginTop: 16 }}>
      {providers.map((provider) => {
        const active = state.activeProviderKey === provider.provider_key;
        const editing = state.editing?.id === provider.id ? state.editing : null;
        return (
          <li
            key={provider.id}
            className={`provider-card ${active ? 'provider-card--active' : ''}`}
            style={{
              padding: 12,
              marginBottom: 10,
              borderRadius: 'var(--radius, 10px)',
              border: `1px solid ${active ? 'var(--accent, #4c6fff)' : 'var(--border, #e5e7eb)'}`,
            }}
          >
            {editing ? (
              <CloudProviderFormCard
                key={`edit-${provider.id}`}
                initial={editing}
                submitting={state.submitting}
                onSubmit={actions.onSubmit}
                onCancel={actions.onCancelEdit}
              />
            ) : (
              <CloudProviderRowView
                provider={provider}
                keyStatus={apiKeyStatus[provider.provider_key]}
                state={state}
                actions={actions}
              />
            )}
          </li>
        );
      })}
    </ul>
  );
}
