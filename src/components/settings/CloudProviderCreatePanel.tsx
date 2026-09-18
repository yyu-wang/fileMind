// 新建提供商的入口区：右上角「+ 添加提供商」按钮 + 展开后的表单卡片。
//
// 从 CloudProviderManager 抽出（原为容器 JSX 里相邻的两段条件渲染）。

import type { CloudProviderUpsertInput } from '@/types/ipc';

import { CloudProviderFormCard } from './CloudProviderFormCard';

interface CloudProviderCreatePanelProps {
  /** 新建表单是否展开 */
  expanded: boolean;
  /** 是否展示「+ 添加提供商」按钮（表单已展开或正在行内编辑时隐藏） */
  showTrigger: boolean;
  /** 提交中 */
  submitting: boolean;
  /** 展开新建表单 */
  onCreate: () => void;
  /** 收起新建表单 */
  onCancel: () => void;
  /** 提交新建表单 */
  onSubmit: (input: CloudProviderUpsertInput) => Promise<void> | void;
}

export function CloudProviderCreatePanel({
  expanded,
  showTrigger,
  submitting,
  onCreate,
  onCancel,
  onSubmit,
}: CloudProviderCreatePanelProps) {
  return (
    <>
      {showTrigger && (
        <div style={{ display: 'flex', justifyContent: 'flex-end', marginBottom: 12 }}>
          <button type="button" className="btn btn--primary btn--sm" onClick={onCreate}>
            + 添加提供商
          </button>
        </div>
      )}

      {expanded && (
        <CloudProviderFormCard submitting={submitting} onSubmit={onSubmit} onCancel={onCancel} />
      )}
    </>
  );
}
