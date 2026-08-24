// 云端 API Key 区块（设置页 · 安全 07-§4 / T7.3）。
//
// 安全约束：完整 Key 只经 setApiKey 写入系统 Keychain，前端任何时刻都拿不到
// 明文——store 仅存 has_key + 末 4 位 hint。输入框只用于录入新 Key（或覆盖），
// 保存成功后即清空；已配置行只回显 `已保存 ····abcd`，删除走掩码确认。

import { useEffect, useState } from 'react';
import { useSettingsStore } from '../../stores/settingsStore';
import type { CloudProvider } from '../../types/ipc';

//: 展示顺序与显示名（与 Rust 端 ALL_PROVIDERS 展开一致）
const PROVIDERS: Array<{ value: CloudProvider; label: string }> = [
  { value: 'Openai', label: 'OpenAI' },
  { value: 'Deepseek', label: 'DeepSeek' },
];

const EMPTY_DRAFT: Record<CloudProvider, string> = { Openai: '', Deepseek: '' };

export function CloudApiKeySection() {
  const apiKeyStatus = useSettingsStore((s) => s.apiKeyStatus);
  const loadApiKeyStatus = useSettingsStore((s) => s.loadApiKeyStatus);
  const setApiKey = useSettingsStore((s) => s.setApiKey);
  const deleteApiKey = useSettingsStore((s) => s.deleteApiKey);

  const [drafts, setDrafts] = useState<Record<CloudProvider, string>>(EMPTY_DRAFT);
  const [busyProvider, setBusyProvider] = useState<CloudProvider | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    void loadApiKeyStatus();
  }, [loadApiKeyStatus]);

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
        云端 API Key
      </h3>
      <p className="settings-section__desc">
        云端模式需配置云服务商 API Key。Key 仅保存在系统钥匙串（Keychain），界面只显示末 4 位。
      </p>
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
