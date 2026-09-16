// 删除提供商的二次确认弹窗。
//
// 从 CloudProviderManager 抽出（原为容器 JSX 尾段的条件渲染）。

import { ConfirmDialog } from '@/components/ui/ConfirmDialog';
import type { CloudProviderRecord } from '@/types/ipc';

interface CloudProviderDeleteDialogProps {
  /** 待删除的提供商 */
  provider: CloudProviderRecord;
  /** 取消（不删除） */
  onCancel: () => void;
  /** 确认删除 */
  onConfirm: () => void;
}

export function CloudProviderDeleteDialog({
  provider,
  onCancel,
  onConfirm,
}: CloudProviderDeleteDialogProps) {
  return (
    <ConfirmDialog
      title="删除云提供商"
      message={`确认删除提供商「${provider.name} (${provider.provider_key})」？删除后历史记录与 API Key 保留，但若需同名新增会被系统拦截。`}
      confirmLabel="删除"
      danger
      loading={false}
      onCancel={onCancel}
      onConfirm={onConfirm}
    />
  );
}
