// 云端 API Key 区块（设置页 · 安全 07-§4 / T7.3）。
//
// 安全约束：完整 Key 只经 setApiKey 写入系统 Keychain，前端任何时刻都拿不到
// 明文——store 仅存 has_key + 末 4 位 hint。输入框只用于录入新 Key（或覆盖），
// 保存成功后即清空；已配置行只回显 `已保存 ····abcd`，删除走掩码确认。
//
// 云端模型：T12 打通后，用户可在设置页选择/输入云端推理模型，持久化到
// AppConfig → Rust chat_stream 覆盖 llm_model → Sidecar Provider 按前缀路由。
// RAG 问答和文件分类均为确定性任务，固定低温度（服务端默认），不对外开放调节。
// 原型 05_交互原型 §设置页 AI 模型配置：.setting-row + .setting-label + .setting-control。

import { useEffect, useState } from 'react';
import { ConfirmDialog } from '../ui/ConfirmDialog';
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
  // 待删除的 provider（ConfirmDialog 二次确认，替代阻塞式 window.confirm）
  const [pendingRemove, setPendingRemove] = useState<CloudProvider | null>(null);
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
            value={modelDraft}
            onChange={(e) => setModelDraft(e.target.value)}
            placeholder="选择或输入模型名"
            aria-label="云端推理模型名"
            style={{ width: 220 }}
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

      {PROVIDERS.map(({ value, label }) => {
        const status = apiKeyStatus[value];
        const busy = busyProvider === value;
        const hasKey = status.has_key;
        return (
          <div key={value} className="setting-row">
            <div className="setting-label">
              <div className="name">
                {label} API Key
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
              <div className="desc">云端模式需要配置 API Key</div>
            </div>
            <div
              className="setting-control"
              style={{ display: 'flex', gap: 8, alignItems: 'center' }}
            >
              <input
                type="password"
                className="input"
                aria-label={`${label} API Key 输入框`}
                placeholder={hasKey ? '输入新 Key 可覆盖' : '粘贴 API Key'}
                value={drafts[value]}
                autoComplete="off"
                disabled={busy}
                onChange={(e) => setDrafts((d) => ({ ...d, [value]: e.target.value }))}
                style={{ width: 220 }}
              />
              <button
                type="button"
                className="btn btn--primary btn--sm"
                disabled={busy || !drafts[value].trim()}
                onClick={() => void save(value)}
              >
                保存
              </button>
              {hasKey && (
                <button
                  type="button"
                  className="btn btn--ghost btn--sm"
                  disabled={busy}
                  onClick={() => setPendingRemove(value)}
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
          message={`确认删除 ${
            PROVIDERS.find((p) => p.value === pendingRemove)?.label ?? pendingRemove
          } 的 API Key？删除后需重新输入。`}
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
