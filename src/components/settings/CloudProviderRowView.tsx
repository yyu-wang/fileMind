// 提供商行的展示态：单选激活 + 信息 + 操作按钮（设置页「云提供商管理」）。
//
// 从 CloudProviderManager 的行内渲染抽出。行契约（状态/动作）也定义在这里——它是这两个
// 类型的消费方，列表、行按钮与 useCloudProviderManager 都从这里引用。

import type { ApiKeyStatus, CloudProviderRecord, CloudProviderUpsertInput } from '@/types/ipc';

import { CloudProviderInfo } from './CloudProviderInfo';
import { CloudProviderRowButtons } from './CloudProviderRowButtons';

/** 列表行的状态（父级一次性传给每一行）。 */
export interface CloudProviderRowState {
  /** 当前激活的提供商 slug */
  activeProviderKey: string;
  /** 正在切换激活的提供商 slug（null 表示无切换在途） */
  activating: string | null;
  /** 正在编辑的提供商（null 表示无；仅 id 匹配的那行内联编辑） */
  editing: CloudProviderRecord | null;
  /** 表单提交中 */
  submitting: boolean;
}

/** 列表行的动作（父级一次性传给每一行）。 */
export interface CloudProviderRowActions {
  /** 激活该提供商（点击单选） */
  onActivate: (providerKey: string) => void;
  /** 进入该行的编辑态 */
  onEdit: (provider: CloudProviderRecord) => void;
  /** 退出编辑态 */
  onCancelEdit: () => void;
  /** 请求删除（先弹二次确认） */
  onRequestDelete: (provider: CloudProviderRecord) => void;
  /** 表单提交（新建 / 编辑共用） */
  onSubmit: (input: CloudProviderUpsertInput) => Promise<void> | void;
}

interface CloudProviderRowViewProps {
  /** 该行对应的提供商 */
  provider: CloudProviderRecord;
  /** 该提供商的 Key 状态 */
  keyStatus: ApiKeyStatus | undefined;
  /** 行状态 */
  state: CloudProviderRowState;
  /** 行动作 */
  actions: CloudProviderRowActions;
}

export function CloudProviderRowView({
  provider,
  keyStatus,
  state,
  actions,
}: CloudProviderRowViewProps) {
  const active = state.activeProviderKey === provider.provider_key;
  const activating = state.activating === provider.provider_key;

  return (
    <div
      style={{
        display: 'grid',
        gridTemplateColumns: '32px 1fr auto',
        gap: 12,
        alignItems: 'center',
      }}
    >
      <label
        style={{
          display: 'inline-flex',
          alignItems: 'center',
          gap: 6,
          cursor: active || activating ? 'default' : 'pointer',
        }}
      >
        <input
          type="radio"
          name="active-cloud-provider"
          value={provider.provider_key}
          checked={active}
          disabled={activating || active}
          onChange={() => actions.onActivate(provider.provider_key)}
          aria-label={`激活 ${provider.name}`}
        />
      </label>

      <CloudProviderInfo provider={provider} keyStatus={keyStatus} active={active} />

      <CloudProviderRowButtons provider={provider} actions={actions} />
    </div>
  );
}
