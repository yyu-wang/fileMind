// P-07：云提供商列表+激活切换+添加/编辑/删除管理组件。
//
// 组件结构：
//   [+ 添加提供商] (默认隐藏表单，点击展开 CloudProviderFormCard)
//   <providers 列表>
//     [单选激活] 名称  slug  base_url  Key 状态(已保存/未配置)
//     [编辑] [删除（内置不可删）]
//   展开编辑时内联显示 CloudProviderFormCard

import { useEffect, useState } from 'react';
import { useSettingsStore } from '../../stores/settingsStore';
import type { CloudProviderRecord } from '../../types/ipc';
import { CloudProviderFormCard } from './CloudProviderFormCard';
import { ConfirmDialog } from '../ui/ConfirmDialog';

export function CloudProviderManager() {
  const cloudProviders = useSettingsStore((s) => s.cloudProviders);
  const loading = useSettingsStore((s) => s.cloudProvidersLoading);
  const loadCloudProviders = useSettingsStore((s) => s.loadCloudProviders);
  const upsertCloudProvider = useSettingsStore((s) => s.upsertCloudProvider);
  const deleteCloudProvider = useSettingsStore((s) => s.deleteCloudProvider);
  const activeCloudProvider = useSettingsStore((s) => s.activeCloudProvider);
  const setActiveCloudProvider = useSettingsStore((s) => s.setActiveCloudProvider);
  const apiKeyStatus = useSettingsStore((s) => s.apiKeyStatus);

  const [showCreate, setShowCreate] = useState(false);
  const [editing, setEditing] = useState<CloudProviderRecord | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const [pendingDelete, setPendingDelete] = useState<CloudProviderRecord | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [activating, setActivating] = useState<string | null>(null);

  useEffect(() => {
    void loadCloudProviders();
  }, [loadCloudProviders]);

  const handleUpsert = async (input: Parameters<typeof upsertCloudProvider>[0]) => {
    setSubmitting(true);
    setError(null);
    try {
      await upsertCloudProvider(input);
      setShowCreate(false);
      setEditing(null);
    } catch (e) {
      setError(e instanceof Error ? e.message : '保存提供商失败');
    } finally {
      setSubmitting(false);
    }
  };

  const handleDelete = async () => {
    if (!pendingDelete) return;
    const victim = pendingDelete;
    setPendingDelete(null);
    try {
      await deleteCloudProvider(victim.provider_key);
    } catch (e) {
      setError(e instanceof Error ? e.message : '删除提供商失败');
    }
  };

  const handleActivate = async (provider_key: string) => {
    if (activeCloudProvider === provider_key) return;
    setActivating(provider_key);
    setError(null);
    try {
      await setActiveCloudProvider(provider_key);
    } catch (e) {
      setError(e instanceof Error ? e.message : '切换激活提供商失败');
    } finally {
      setActivating(null);
    }
  };

  return (
    <section className="settings-section" aria-labelledby="settings-cloud-providers-title">
      <h3 id="settings-cloud-providers-title" className="settings-section__title">
        ☁️ 云提供商管理
      </h3>
      <p className="section-desc">
        添加任何 OpenAI 兼容的云服务（通义、千问、Claude、Moonshot
        等）。激活后将作为云端推理的上游， API Key 在下方「AI 模型配置」单独写入系统 Keychain。
      </p>

      {!showCreate && !editing && (
        <div style={{ display: 'flex', justifyContent: 'flex-end', marginBottom: 12 }}>
          <button
            type="button"
            className="btn btn--primary btn--sm"
            onClick={() => setShowCreate(true)}
          >
            + 添加提供商
          </button>
        </div>
      )}

      {showCreate && (
        <CloudProviderFormCard
          submitting={submitting}
          onSubmit={handleUpsert}
          onCancel={() => setShowCreate(false)}
        />
      )}

      {loading && cloudProviders.length === 0 && (
        <div className="settings-card settings-card--skeleton">加载提供商列表...</div>
      )}

      {!loading && cloudProviders.length === 0 && !showCreate && (
        <div className="settings-empty-state">暂无提供商。点击右上角「+ 添加提供商」开始配置。</div>
      )}

      <ul className="provider-list" style={{ listStyle: 'none', padding: 0, marginTop: 16 }}>
        {cloudProviders.map((p) => {
          const keyStatus = apiKeyStatus[p.provider_key];
          const hasKey = keyStatus?.has_key;
          const active = activeCloudProvider === p.provider_key;
          const activatingNow = activating === p.provider_key;
          return (
            <li
              key={p.id}
              className={`provider-card ${active ? 'provider-card--active' : ''}`}
              style={{
                padding: 12,
                marginBottom: 10,
                borderRadius: 'var(--radius, 10px)',
                border: `1px solid ${active ? 'var(--accent, #4c6fff)' : 'var(--border, #e5e7eb)'}`,
              }}
            >
              {editing && editing.id === p.id ? (
                <CloudProviderFormCard
                  key={`edit-${p.id}`}
                  initial={editing}
                  submitting={submitting}
                  onSubmit={handleUpsert}
                  onCancel={() => setEditing(null)}
                />
              ) : (
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
                      cursor: active || activatingNow ? 'default' : 'pointer',
                    }}
                  >
                    <input
                      type="radio"
                      name="active-cloud-provider"
                      value={p.provider_key}
                      checked={active}
                      disabled={activatingNow || active}
                      onChange={() => void handleActivate(p.provider_key)}
                      aria-label={`激活 ${p.name}`}
                    />
                  </label>

                  <div className="provider-card__info">
                    <div
                      style={{ display: 'flex', alignItems: 'center', gap: 8, flexWrap: 'wrap' }}
                    >
                      <strong>{p.name}</strong>
                      <span className="tag tag-gray" style={{ fontSize: 11 }}>
                        {p.provider_key}
                      </span>
                      {p.is_builtin && (
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
                          Key {keyStatus.hint || '已保存'}
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
                      {p.base_url}
                    </div>
                    {p.remark && (
                      <div
                        style={{
                          marginTop: 2,
                          fontSize: 12,
                          color: 'var(--text-muted, #6b7280)',
                        }}
                      >
                        {p.remark}
                      </div>
                    )}
                  </div>

                  <div style={{ display: 'flex', gap: 6 }}>
                    <button
                      type="button"
                      className="btn btn--ghost btn--sm"
                      onClick={() => {
                        setEditing(p);
                        setShowCreate(false);
                      }}
                    >
                      编辑
                    </button>
                    <button
                      type="button"
                      className="btn btn--ghost btn--sm"
                      disabled={p.is_builtin}
                      title={p.is_builtin ? '内置提供商不可删除' : '删除该提供商'}
                      onClick={() => setPendingDelete(p)}
                    >
                      删除
                    </button>
                  </div>
                </div>
              )}
            </li>
          );
        })}
      </ul>

      {error && (
        <p className="settings-section__error" role="alert" style={{ marginTop: 12 }}>
          {error}
        </p>
      )}

      {pendingDelete && (
        <ConfirmDialog
          title="删除云提供商"
          message={`确认删除提供商「${pendingDelete.name} (${pendingDelete.provider_key})」？删除后历史记录与 API Key 保留，但若需同名新增会被系统拦截。`}
          confirmLabel="删除"
          danger
          loading={false}
          onCancel={() => setPendingDelete(null)}
          onConfirm={() => void handleDelete()}
        />
      )}
    </section>
  );
}
