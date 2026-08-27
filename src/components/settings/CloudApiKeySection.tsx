// 云端 API Key 区块（设置页 · 安全 07-§4 / T7.3）。
//
// 安全约束：完整 Key 只经 setApiKey 写入系统 Keychain，前端任何时刻都拿不到
// 明文——store 仅存 has_key + 末 4 位 hint。输入框只用于录入新 Key（或覆盖），
// 保存成功后即清空；已配置行只回显 `已保存 ····abcd`，删除走掩码确认。
//
// 云端模型：T12 打通后，用户可在设置页选择/输入云端推理模型，持久化到
// AppConfig → Rust chat_stream 覆盖 llm_model → Sidecar Provider 按前缀路由。
// RAG 问答和文件分类均为确定性任务，固定低温度（服务端默认），不对外开放调节。

import { useEffect, useState } from 'react';
import { useSettingsStore } from '../../stores/settingsStore';
import type { CloudProvider } from '../../types/ipc';

//: 展示顺序与显示名（与 Rust 端 ALL_PROVIDERS 展开一致）
const PROVIDERS: Array<{ value: CloudProvider; label: string }> = [
  { value: 'Openai', label: 'OpenAI' },
  { value: 'Deepseek', label: 'DeepSeek' },
];

//: 云端模型预设列表（按当前主流 API 整理，用户也可手动输入自定义模型名）
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
];

const EMPTY_DRAFT: Record<CloudProvider, string> = { Openai: '', Deepseek: '' };

export function CloudApiKeySection() {
  const apiKeyStatus = useSettingsStore((s) => s.apiKeyStatus);
  const loadApiKeyStatus = useSettingsStore((s) => s.loadApiKeyStatus);
  const setApiKey = useSettingsStore((s) => s.setApiKey);
  const deleteApiKey = useSettingsStore((s) => s.deleteApiKey);
  const cloudModel = useSettingsStore((s) => s.cloudModel);
  const updateConfig = useSettingsStore((s) => s.updateConfig);

  const [drafts, setDrafts] = useState<Record<CloudProvider, string>>(EMPTY_DRAFT);
  const [busyProvider, setBusyProvider] = useState<CloudProvider | null>(null);
  const [error, setError] = useState<string | null>(null);
  // 初始化从 store 读取：后续由用户编辑，不随 store 外部变更自动覆盖
  const [modelDraft, setModelDraft] = useState(() => cloudModel || 'deepseek-chat');
  const [savingModel, setSavingModel] = useState(false);

  useEffect(() => {
    void loadApiKeyStatus();
  }, [loadApiKeyStatus]);

  const saveModel = async () => {
    const trimmedModel = modelDraft.trim();
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

  const save = async (provider: CloudProvider) => {
    const key = drafts[provider].trim();
    if (!key) {
      setError('请输入 API Key 后再保存');
      return;
    }
    setBusyProvider(provider);
    setError(null);
    try {
      await setApiKey(provider, key);
      // 保存成功后清空输入框，后续只显示掩码 hint
      setDrafts((d) => ({ ...d, [provider]: '' }));
    } catch (e) {
      setError(e instanceof Error ? e.message : '保存 API Key 失败');
    } finally {
      setBusyProvider(null);
    }
  };

  const remove = async (provider: CloudProvider) => {
    // FE-m13：删除 API Key 需二次确认，避免误操作丢失凭据
    const displayName = PROVIDERS.find((p) => p.value === provider)?.label ?? provider;
    if (!window.confirm(`确认删除 ${displayName} 的 API Key？删除后需重新输入。`)) {
      return;
    }
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

  return (
    <section className="settings-section" aria-labelledby="settings-api-key-title">
      <h3 id="settings-api-key-title" className="settings-section__title">
        云端 API Key
      </h3>
      <p className="settings-section__desc">
        云端模式需配置云服务商 API Key。Key 仅保存在系统钥匙串（Keychain），界面只显示末 4 位。
      </p>
      {/* T12 打通：云端模型可编辑，持久化到 AppConfig */}
      <div className="settings-row">
        <div className="settings-field">
          <label htmlFor="cloud-model-select" className="settings-field__label">
            云端模型
          </label>
          <div className="settings-model-combo">
            <input
              id="cloud-model-select"
              className="settings-select settings-model-input"
              list="cloud-model-options"
              value={modelDraft}
              onChange={(e) => setModelDraft(e.target.value)}
              placeholder="选择或输入模型名"
              aria-label="云端推理模型名"
            />
            <datalist id="cloud-model-options">
              {CLOUD_MODEL_OPTIONS.map((opt) => (
                <option key={opt.value} value={opt.value}>
                  {opt.label}
                </option>
              ))}
            </datalist>
          </div>
          <p className="settings-field__hint">
            选择预设或手动输入模型名（如 gpt-4o / deepseek-chat / claude-3-5-sonnet）
          </p>
        </div>
      </div>
      <div className="settings-row settings-row--actions">
        <button
          type="button"
          className="btn btn--primary btn--sm"
          disabled={savingModel}
          onClick={() => void saveModel()}
        >
          {savingModel ? '保存中...' : '保存模型设置'}
        </button>
      </div>
      {PROVIDERS.map(({ value, label }) => {
        const status = apiKeyStatus[value];
        const busy = busyProvider === value;
        const hasKey = status.has_key;
        return (
          <div key={value} className="settings-row">
            <span className="settings-api-key__provider">
              {label}
              {hasKey ? (
                <span className="settings-mode-badge" title="已配置 API Key">
                  已保存 {status.hint}
                </span>
              ) : (
                <span className="settings-mode-badge settings-mode-badge--empty">未配置</span>
              )}
            </span>
            <input
              type="password"
              className="settings-api-key__input"
              aria-label={`${label} API Key 输入框`}
              placeholder={hasKey ? '输入新 Key 可覆盖' : '粘贴 API Key'}
              value={drafts[value]}
              autoComplete="off"
              disabled={busy}
              onChange={(e) => setDrafts((d) => ({ ...d, [value]: e.target.value }))}
            />
            <button
              type="button"
              className="btn btn--primary"
              disabled={busy || !drafts[value].trim()}
              onClick={() => void save(value)}
            >
              保存
            </button>
            {hasKey && (
              <button
                type="button"
                className="btn btn--ghost"
                disabled={busy}
                onClick={() => void remove(value)}
              >
                删除
              </button>
            )}
          </div>
        );
      })}
      {error && (
        <p className="settings-section__error" role="alert">
          {error}
        </p>
      )}
    </section>
  );
}
