// 云端 API Key 区块（设置页 · 安全 07-§4 / T7.3）。
//
// 安全约束：完整 Key 只经 setApiKey 写入系统 Keychain，前端任何时刻都拿不到
// 明文——store 仅存 has_key + 末 4 位 hint。输入框只用于录入新 Key（或覆盖），
// 保存成功后即清空；已配置行只回显 `已保存 ····abcd`，删除走掩码确认。
//
// P-07 改造：Provider 列表不再硬编码，统一从 store.cloudProviders 取
// （Rust DB cloud_providers 表），用户在上方「云提供商管理」卡片添加。
// 模型名保留常用预设，用户仍可手动输入任意模型名。

import { useEffect, useMemo, useState } from 'react';
import { ConfirmDialog } from '../ui/ConfirmDialog';
import { useSettingsStore } from '../../stores/settingsStore';

//: 云端模型预设列表（仅作下拉示例，不绑定提供商）
const CLOUD_MODEL_OPTIONS = [
  // DeepSeek
  { value: 'deepseek-chat', label: 'DeepSeek Chat' },
  { value: 'deepseek-reasoner', label: 'DeepSeek Reasoner' },
  { value: 'deepseek-v3', label: 'DeepSeek V3' },
  // OpenAI
  { value: 'gpt-4o', label: 'GPT-4o' },
  { value: 'gpt-4o-mini', label: 'GPT-4o Mini' },
  { value: 'gpt-4.1', label: 'GPT-4.1' },
  { value: 'gpt-4.1-mini', label: 'GPT-4.1 Mini' },
  // Anthropic（通过兼容代理接入时使用）
  { value: 'claude-3-5-sonnet', label: 'Claude 3.5 Sonnet' },
  { value: 'claude-3-opus', label: 'Claude 3 Opus' },
  // Qwen
  { value: 'qwen-plus', label: 'Qwen Plus' },
  { value: 'qwen-turbo', label: 'Qwen Turbo' },
  // Moonshot
  { value: 'moonshot-v1-8k', label: 'Moonshot v1 8K' },
  // GLM
  { value: 'glm-4', label: 'GLM-4' },
];

export function CloudApiKeySection() {
  const apiKeyStatus = useSettingsStore((s) => s.apiKeyStatus);
  const cloudProviders = useSettingsStore((s) => s.cloudProviders);
  const loadApiKeyStatus = useSettingsStore((s) => s.loadApiKeyStatus);
  const loadCloudProviders = useSettingsStore((s) => s.loadCloudProviders);
  const setApiKey = useSettingsStore((s) => s.setApiKey);
  const deleteApiKey = useSettingsStore((s) => s.deleteApiKey);
  const cloudModel = useSettingsStore((s) => s.cloudModel);
  const updateConfig = useSettingsStore((s) => s.updateConfig);

  const [drafts, setDrafts] = useState<Record<string, string>>({});
  const [busyProvider, setBusyProvider] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [pendingRemove, setPendingRemove] = useState<string | null>(null);
  const [modelDraft, setModelDraft] = useState(() => cloudModel || '');
  const [savingModel, setSavingModel] = useState(false);

  useEffect(() => {
    void (async () => {
      // Provider 列表是 Key 行的基准；先 loadCloudProviders（其内部会连带
      // 调 loadApiKeyStatus），保证界面至少渲染内置的 2 条。
      if (cloudProviders.length === 0) {
        await loadCloudProviders();
      } else {
        await loadApiKeyStatus();
      }
    })();
  }, [loadCloudProviders, loadApiKeyStatus, cloudProviders.length]);

  // Store 更新时，如果当前输入框仍为空则回填一次；不使用 setState-in-effect，
  // 而是把赋值推到 setTimeout(0) 延后执行，避免级联重渲染。
  if (cloudModel && !modelDraft) {
    window.setTimeout(() => setModelDraft(cloudModel), 0);
  }
  // 取派生显示值，保证至少展示 store 已有值
  const displayModelDraft = modelDraft || cloudModel || '';

  const providerOrder = useMemo(() => cloudProviders, [cloudProviders]);

  const saveModel = async () => {
    const trimmedModel = displayModelDraft.trim();
    if (!trimmedModel) {
      setError('云端模型名不能为空');
      return;
    }
    setSavingModel(true);
    setError(null);
    try {
      await updateConfig({ cloud_model: trimmedModel });
    } catch (e) {
      setError(e instanceof Error ? e.message : '保存云端模型配置失败');
    } finally {
      setSavingModel(false);
    }
  };

  const save = async (provider: string) => {
    const key = (drafts[provider] ?? '').trim();
    if (!key) {
      setError('请输入 API Key 后再保存');
      return;
    }
    setBusyProvider(provider);
    setError(null);
    try {
      await setApiKey(provider, key);
      setDrafts((d) => ({ ...d, [provider]: '' }));
    } catch (e) {
      setError(e instanceof Error ? e.message : '保存 API Key 失败');
    } finally {
      setBusyProvider(null);
    }
  };

  const remove = async (provider: string) => {
    setBusyProvider(provider);
    setError(null);
    try {
      await deleteApiKey(provider);
    } catch (e) {
      setError(e instanceof Error ? e.message : '删除 API Key 失败');
    } finally {
      setBusyProvider(null);
    }
  };

  if (providerOrder.length === 0) {
    return (
      <section className="settings-section" aria-labelledby="settings-api-key-title-empty">
        <h3 id="settings-api-key-title-empty" className="settings-section__title">
          🤖 AI 模型配置
        </h3>
        <p className="section-desc">
          请先在上方「云提供商管理」添加一个提供商，再回来配置 API Key 与模型。
        </p>
      </section>
    );
  }

  return (
    <section className="settings-section" aria-labelledby="settings-api-key-title">
      <h3 id="settings-api-key-title" className="settings-section__title">
        🤖 AI 模型配置
      </h3>
      <p className="section-desc">
        配置云端推理模型与 API Key。Key 仅保存在系统钥匙串（Keychain），界面只显示末 4 位。
      </p>

      <div className="setting-row">
        <div className="setting-label">
          <div className="name">云端模型</div>
          <div className="desc">API 推理使用的模型（选择预设或手动输入）</div>
        </div>
        <div className="setting-control">
          <input
            className="input"
            list="cloud-model-options"
            value={displayModelDraft}
            onChange={(e) => setModelDraft(e.target.value)}
            placeholder="选择或输入模型名"
            aria-label="云端推理模型名"
            style={{ width: 260 }}
          />
          <datalist id="cloud-model-options">
            {CLOUD_MODEL_OPTIONS.map((opt) => (
              <option key={opt.value} value={opt.value}>
                {opt.label}
              </option>
            ))}
          </datalist>
        </div>
      </div>
      <div className="setting-row" style={{ borderBottom: 'none', paddingTop: 0 }}>
        <div className="setting-label" />
        <div className="setting-control">
          <button
            type="button"
            className="btn btn--primary btn--sm"
            disabled={savingModel}
            onClick={() => void saveModel()}
          >
            {savingModel ? '保存中...' : '保存模型设置'}
          </button>
        </div>
      </div>

      {providerOrder.map((p) => {
        const status = apiKeyStatus[p.provider_key] ?? {
          provider: p.provider_key,
          has_key: false,
          hint: '',
        };
        const busy = busyProvider === p.provider_key;
        const hasKey = status.has_key;
        const draft = drafts[p.provider_key] ?? '';
        return (
          <div key={p.provider_key} className="setting-row">
            <div className="setting-label">
              <div className="name">
                {p.name} API Key
                <span className="tag tag-gray" style={{ marginLeft: 8, fontSize: 10 }}>
                  {p.provider_key}
                </span>
                {hasKey ? (
                  <span
                    className="tag tag-green"
                    style={{ marginLeft: 8, fontSize: 10 }}
                    title="已配置 API Key"
                  >
                    已保存 {status.hint}
                  </span>
                ) : (
                  <span className="tag tag-gray" style={{ marginLeft: 8, fontSize: 10 }}>
                    未配置
                  </span>
                )}
              </div>
              <div className="desc" style={{ fontSize: 11 }}>
                {p.base_url}
              </div>
            </div>
            <div
              className="setting-control"
              style={{ display: 'flex', gap: 8, alignItems: 'center' }}
            >
              <input
                type="password"
                className="input"
                aria-label={`${p.name} API Key 输入框`}
                placeholder={hasKey ? '输入新 Key 可覆盖' : '粘贴 API Key'}
                value={draft}
                autoComplete="off"
                disabled={busy}
                onChange={(e) => setDrafts((d) => ({ ...d, [p.provider_key]: e.target.value }))}
                style={{ width: 260 }}
              />
              <button
                type="button"
                className="btn btn--primary btn--sm"
                disabled={busy || !draft.trim()}
                onClick={() => void save(p.provider_key)}
              >
                保存
              </button>
              {hasKey && (
                <button
                  type="button"
                  className="btn btn--ghost btn--sm"
                  disabled={busy}
                  onClick={() => setPendingRemove(p.provider_key)}
                >
                  删除
                </button>
              )}
            </div>
          </div>
        );
      })}
      {error && (
        <p className="settings-section__error" role="alert">
          {error}
        </p>
      )}

      {pendingRemove !== null && (
        <ConfirmDialog
          title="删除 API Key"
          message={`确认删除「${
            providerOrder.find((p) => p.provider_key === pendingRemove)?.name ?? pendingRemove
          }」的 API Key？删除后需重新输入。`}
          confirmLabel="删除"
          danger
          loading={busyProvider === pendingRemove}
          onCancel={() => setPendingRemove(null)}
          onConfirm={() => {
            const provider = pendingRemove;
            setPendingRemove(null);
            void remove(provider);
          }}
        />
      )}
    </section>
  );
}
