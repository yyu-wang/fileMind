// 云端 API Key 区块（设置页 · 安全 07-§4 / T7.3）。
//
// 安全约束：完整 Key 只经 setApiKey 写入系统 Keychain，前端任何时刻都拿不到
// 明文——store 仅存 has_key + 末 4 位 hint。输入框只用于录入新 Key（或覆盖），
// 保存成功后即清空；已配置行只回显 `已保存 ····abcd`，删除走掩码确认。
//
// P-07 改造：Provider 列表不再硬编码，统一从 store.cloudProviders 取
// （Rust DB cloud_providers 表），用户在上方「云提供商管理」卡片添加。
// 模型名保留常用预设，用户仍可手动输入任意模型名。
//
// 状态与 handler 收在 useCloudApiKeySection，视觉区域拆为 CloudModelSettingRow（模型行）
// / CloudApiKeyList（Key 行）/ CloudApiKeyEmptyState（无 Provider 占位）
// / CloudApiKeyRemoveDialog（删除确认）。

import { CloudApiKeyEmptyState } from './CloudApiKeyEmptyState';
import { CloudApiKeyList } from './CloudApiKeyList';
import { CloudApiKeyRemoveDialog } from './CloudApiKeyRemoveDialog';
import { CloudModelSettingRow } from './CloudModelSettingRow';
import { useCloudApiKeySection } from './useCloudApiKeySection';

export function CloudApiKeySection() {
  const section = useCloudApiKeySection();

  if (section.providers.length === 0) {
    return <CloudApiKeyEmptyState />;
  }

  return (
    <section className="settings-section" aria-labelledby="settings-api-key-title">
      <h3 id="settings-api-key-title" className="settings-section__title">
        🤖 AI 模型配置
      </h3>
      <p className="section-desc">
        配置云端推理模型与 API Key。Key 仅保存在系统钥匙串（Keychain），界面只显示末 4 位。
      </p>

      <CloudModelSettingRow
        value={section.displayModelDraft}
        saving={section.savingModel}
        onChange={section.setModelDraft}
        onSave={() => void section.saveModel()}
      />

      <CloudApiKeyList
        providers={section.providers}
        apiKeyStatus={section.apiKeyStatus}
        drafts={section.drafts}
        busyProvider={section.busyProvider}
        onDraftChange={section.changeDraft}
        onSave={(providerKey) => void section.save(providerKey)}
        onRequestRemove={section.setPendingRemove}
      />

      {section.error && (
        <p className="settings-section__error" role="alert">
          {section.error}
        </p>
      )}

      {section.pendingRemove !== null && (
        <CloudApiKeyRemoveDialog
          providerName={section.pendingRemoveName}
          loading={section.busyProvider === section.pendingRemove}
          onCancel={() => section.setPendingRemove(null)}
          onConfirm={() => void section.confirmRemove()}
        />
      )}
    </section>
  );
}
