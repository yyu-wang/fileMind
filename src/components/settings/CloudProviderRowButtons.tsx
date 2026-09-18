// 提供商行的操作按钮：编辑 / 删除（内置提供商不可删）。
//
// 从 CloudProviderManager 的行内渲染抽出（原为该行 JSX 的尾段）。

import type { CloudProviderRecord } from '@/types/ipc';

import type { CloudProviderRowActions } from './CloudProviderRowView';

interface CloudProviderRowButtonsProps {
  /** 该行对应的提供商 */
  provider: CloudProviderRecord;
  /** 行动作 */
  actions: CloudProviderRowActions;
}

export function CloudProviderRowButtons({ provider, actions }: CloudProviderRowButtonsProps) {
  return (
    <div style={{ display: 'flex', gap: 6 }}>
      <button
        type="button"
        className="btn btn--ghost btn--sm"
        onClick={() => actions.onEdit(provider)}
      >
        编辑
      </button>
      <button
        type="button"
        className="btn btn--ghost btn--sm"
        disabled={provider.is_builtin}
        title={provider.is_builtin ? '内置提供商不可删除' : '删除该提供商'}
        onClick={() => actions.onRequestDelete(provider)}
      >
        删除
      </button>
    </div>
  );
}
