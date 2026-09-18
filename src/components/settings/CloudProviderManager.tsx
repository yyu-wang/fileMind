// P-07：云提供商列表+激活切换+添加/编辑/删除管理组件。
//
// 组件结构：
//   [+ 添加提供商] (默认隐藏表单，点击展开 CloudProviderFormCard)
//   <providers 列表>
//     [单选激活] 名称  slug  base_url  Key 状态(已保存/未配置)
//     [编辑] [删除（内置不可删）]
//   展开编辑时内联显示 CloudProviderFormCard
//
// 状态与 handler 收在 useCloudProviderManager，视觉区域拆为 CloudProviderCreatePanel（新建
// 入口）/ CloudProviderList（列表 + 行）/ CloudProviderDeleteDialog（删除确认）。

import { CloudProviderCreatePanel } from './CloudProviderCreatePanel';
import { CloudProviderDeleteDialog } from './CloudProviderDeleteDialog';
import { CloudProviderList } from './CloudProviderList';
import { useCloudProviderManager } from './useCloudProviderManager';

export function CloudProviderManager() {
  const manager = useCloudProviderManager();

  return (
    <section className="settings-section" aria-labelledby="settings-cloud-providers-title">
      <h3 id="settings-cloud-providers-title" className="settings-section__title">
        ☁️ 云提供商管理
      </h3>
      <p className="section-desc">
        添加任何 OpenAI 兼容的云服务（通义、千问、Claude、Moonshot
        等）。激活后将作为云端推理的上游， API Key 在下方「AI 模型配置」单独写入系统 Keychain。
      </p>

      <CloudProviderCreatePanel
        expanded={manager.showCreate}
        showTrigger={!manager.showCreate && manager.editing === null}
        submitting={manager.submitting}
        onCreate={manager.startCreate}
        onCancel={manager.cancelCreate}
        onSubmit={manager.submitUpsert}
      />

      {manager.loading && manager.providers.length === 0 && (
        <div className="settings-card settings-card--skeleton">加载提供商列表...</div>
      )}

      {!manager.loading && manager.providers.length === 0 && !manager.showCreate && (
        <div className="settings-empty-state">暂无提供商。点击右上角「+ 添加提供商」开始配置。</div>
      )}

      <CloudProviderList
        providers={manager.providers}
        apiKeyStatus={manager.apiKeyStatus}
        state={manager.rowState}
        actions={manager.rowActions}
      />

      {manager.error && (
        <p className="settings-section__error" role="alert" style={{ marginTop: 12 }}>
          {manager.error}
        </p>
      )}

      {manager.pendingDelete && (
        <CloudProviderDeleteDialog
          provider={manager.pendingDelete}
          onCancel={manager.cancelDelete}
          onConfirm={() => void manager.confirmDelete()}
        />
      )}
    </section>
  );
}
