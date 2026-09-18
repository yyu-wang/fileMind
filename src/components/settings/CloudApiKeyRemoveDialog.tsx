// 删除 API Key 的二次确认弹窗。
//
// 从 CloudApiKeySection 抽出（原为容器 JSX 尾段的条件渲染）。

import { ConfirmDialog } from '@/components/ui/ConfirmDialog';

interface CloudApiKeyRemoveDialogProps {
  /** 待删除 Provider 的显示名（找不到时由调用方回退为 slug） */
  providerName: string;
  /** 删除请求在途 */
  loading: boolean;
  /** 取消（不删除） */
  onCancel: () => void;
  /** 确认删除 */
  onConfirm: () => void;
}

export function CloudApiKeyRemoveDialog({
  providerName,
  loading,
  onCancel,
  onConfirm,
}: CloudApiKeyRemoveDialogProps) {
  return (
    <ConfirmDialog
      title="删除 API Key"
      message={`确认删除「${providerName}」的 API Key？删除后需重新输入。`}
      confirmLabel="删除"
      danger
      loading={loading}
      onCancel={onCancel}
      onConfirm={onConfirm}
    />
  );
}
