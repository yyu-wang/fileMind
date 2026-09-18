// 单个 Provider 的 API Key 行：掩码状态标签 + 输入与操作区。
//
// 从 CloudApiKeySection 的列表回调抽出（原为一行 72 行的 map 回调体）。Key 行契约
// （行状态 / 行动作）也定义在这里——它是这两个类型的消费方，列表与输入控件都从这里引用。

import type { ApiKeyStatus, CloudProviderRecord } from '@/types/ipc';

import { CloudApiKeyRowControls } from './CloudApiKeyRowControls';

/** Key 状态缺失时的兜底（apiKeyStatus 初始为空对象，首帧可能没有条目）。 */
const EMPTY_API_KEY_STATUS: ApiKeyStatus = { provider: '', has_key: false, hint: '' };

/** Key 行的动作。 */
export interface CloudApiKeyRowActions {
  /** 输入框草稿变化 */
  onChangeDraft: (value: string) => void;
  /** 保存该行 Key */
  onSave: () => void;
  /** 请求删除该行 Key（先弹二次确认） */
  onRequestRemove: () => void;
}

interface CloudApiKeyRowProps {
  /** 该行对应的 Provider */
  provider: CloudProviderRecord;
  /** 该 Provider 的 Key 状态（store 中可能缺条目，缺失时按未配置渲染） */
  status: ApiKeyStatus | undefined;
  /** 输入框草稿与在途状态 */
  row: { draft: string; busy: boolean };
  /** 行动作 */
  actions: CloudApiKeyRowActions;
}

export function CloudApiKeyRow({ provider, status, row, actions }: CloudApiKeyRowProps) {
  const current = status ?? EMPTY_API_KEY_STATUS;
  const hasKey = current.has_key;

  return (
    <div className="setting-row">
      <div className="setting-label">
        <div className="name">
          {provider.name} API Key
          <span className="tag tag-gray" style={{ marginLeft: 8, fontSize: 10 }}>
            {provider.provider_key}
          </span>
          {hasKey ? (
            <span
              className="tag tag-green"
              style={{ marginLeft: 8, fontSize: 10 }}
              title="已配置 API Key"
            >
              已保存 {current.hint}
            </span>
          ) : (
            <span className="tag tag-gray" style={{ marginLeft: 8, fontSize: 10 }}>
              未配置
            </span>
          )}
        </div>
        <div className="desc" style={{ fontSize: 11 }}>
          {provider.base_url}
        </div>
      </div>
      <CloudApiKeyRowControls
        provider={provider}
        hasKey={hasKey}
        draft={row.draft}
        busy={row.busy}
        actions={actions}
      />
    </div>
  );
}
