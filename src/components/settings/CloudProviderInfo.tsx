// 提供商行的信息区：名称、slug、内置/当前使用/Key 状态标签 + Base URL + 备注。
//
// 从 CloudProviderManager 的行内渲染抽出（原为一行 50 行 JSX 的中段）。

import type { ApiKeyStatus, CloudProviderRecord } from '@/types/ipc';

interface CloudProviderInfoProps {
  /** 该行对应的提供商 */
  provider: CloudProviderRecord;
  /** 该提供商的 Key 状态（store 中可能缺条目，缺失时按未配置展示） */
  keyStatus: ApiKeyStatus | undefined;
  /** 是否为当前激活的提供商 */
  active: boolean;
}

export function CloudProviderInfo({ provider, keyStatus, active }: CloudProviderInfoProps) {
  const hasKey = keyStatus?.has_key;

  return (
    <div className="provider-card__info">
      <div style={{ display: 'flex', alignItems: 'center', gap: 8, flexWrap: 'wrap' }}>
        <strong>{provider.name}</strong>
        <span className="tag tag-gray" style={{ fontSize: 11 }}>
          {provider.provider_key}
        </span>
        {provider.is_builtin && (
          <span className="tag tag-green" style={{ fontSize: 11 }}>
            内置
          </span>
        )}
        {active && (
          <span className="tag tag-green" style={{ fontSize: 11 }}>
            当前使用
          </span>
        )}
        {hasKey ? (
          <span className="tag tag-green" style={{ fontSize: 11 }}>
            Key {keyStatus?.hint || '已保存'}
          </span>
        ) : (
          <span className="tag tag-gray" style={{ fontSize: 11 }}>
            未配置 Key
          </span>
        )}
      </div>
      <div
        style={{
          marginTop: 4,
          fontSize: 12,
          color: 'var(--text-muted, #6b7280)',
          fontFamily: 'var(--font-mono, monospace)',
          wordBreak: 'break-all',
        }}
      >
        {provider.base_url}
      </div>
      {provider.remark && (
        <div
          style={{
            marginTop: 2,
            fontSize: 12,
            color: 'var(--text-muted, #6b7280)',
          }}
        >
          {provider.remark}
        </div>
      )}
    </div>
  );
}
