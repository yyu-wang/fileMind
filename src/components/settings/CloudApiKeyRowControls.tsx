// Key 行的输入与操作区：密码输入框 + 保存 + 删除。
//
// 从 CloudApiKeyRow 抽出——输入框属性与三个按钮占了大半体积，单独成组件后行组件
// 只留左侧的标签信息区。

import type { CloudProviderRecord } from '@/types/ipc';

import type { CloudApiKeyRowActions } from './CloudApiKeyRow';

interface CloudApiKeyRowControlsProps {
  /** 该行对应的 Provider */
  provider: CloudProviderRecord;
  /** 是否已配置 Key（决定占位文案与是否展示删除） */
  hasKey: boolean;
  /** 输入框草稿 */
  draft: string;
  /** 该行有请求在途 */
  busy: boolean;
  /** 行动作 */
  actions: CloudApiKeyRowActions;
}

export function CloudApiKeyRowControls({
  provider,
  hasKey,
  draft,
  busy,
  actions,
}: CloudApiKeyRowControlsProps) {
  return (
    <div className="setting-control" style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
      <input
        type="password"
        className="input"
        aria-label={`${provider.name} API Key 输入框`}
        placeholder={hasKey ? '输入新 Key 可覆盖' : '粘贴 API Key'}
        value={draft}
        autoComplete="off"
        disabled={busy}
        onChange={(e) => actions.onChangeDraft(e.target.value)}
        style={{ width: 260 }}
      />
      <button
        type="button"
        className="btn btn--primary btn--sm"
        disabled={busy || !draft.trim()}
        onClick={actions.onSave}
      >
        保存
      </button>
      {hasKey && (
        <button
          type="button"
          className="btn btn--ghost btn--sm"
          disabled={busy}
          onClick={actions.onRequestRemove}
        >
          删除
        </button>
      )}
    </div>
  );
}
