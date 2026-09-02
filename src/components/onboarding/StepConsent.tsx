// 步骤二：云端知情同意书（对齐交互原型 §Onboarding Step 2，07 规范 §隐私合规）。
//
// 仅云端模式显示，勾选 + 确认后调用 signCloudConsent。
// T7.5 滚动到底门控：未滚读完同意书前 checkbox 禁用，勾选后才能确认。

import { useEffect, useState } from 'react';
import { CLOUD_CONSENT_VERSION } from '../../lib/consent';
import { useSettingsStore } from '../../stores/settingsStore';
import type { CloudProvider } from '../../types/ipc';
import { ConsentAgreement } from '../consent/ConsentAgreement';

interface StepConsentProps {
  onConfirm: (provider: CloudProvider) => void;
  onBack: () => void;
}

export function StepConsent({ onConfirm, onBack }: StepConsentProps) {
  const cloudProviders = useSettingsStore((s) => s.cloudProviders);
  const loadCloudProviders = useSettingsStore((s) => s.loadCloudProviders);
  const defaultKey = cloudProviders[0]?.provider_key ?? 'openai';
  const [agreed, setAgreed] = useState(false);
  const [bottomReached, setBottomReached] = useState(false);
  const [provider, setProvider] = useState<CloudProvider>(defaultKey as CloudProvider);

  useEffect(() => {
    if (cloudProviders.length === 0) {
      void loadCloudProviders();
    } else {
      const validKeys = new Set(cloudProviders.map((p) => p.provider_key));
      if (!validKeys.has(provider)) {
        window.setTimeout(() => setProvider(cloudProviders[0].provider_key as CloudProvider), 0);
      }
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [cloudProviders.length]);

  return (
    <div>
      <h3>隐私知情同意书</h3>

      {/* callout.warn 警告框（对齐交互原型） */}
      <div className="callout warn">
        <div className="callout-title" style={{ color: 'var(--warn)' }}>
          ⚠️ 请仔细阅读以下内容
        </div>
        <p>
          <strong>选择云端模式意味着：</strong>
        </p>
        <ul>
          <li>
            你的<strong>文件内容</strong>将被发送到第三方 AI 服务提供商
          </li>
          <li>这些内容将在对方服务器上处理以生成 AI 回答</li>
          <li>虽然提供商有保密政策，但数据已离开你的设备</li>
        </ul>
        <p>
          <strong>你可以随时撤回同意：</strong>在设置中撤回后，应用自动切换回本地模式。
        </p>
      </div>

      <ConsentAgreement version={CLOUD_CONSENT_VERSION} onBottomReached={setBottomReached} />

      <div className="provider-select">
        <label className="field-label" htmlFor="provider-select">
          选择云端提供商：
        </label>
        <select
          id="provider-select"
          className="input"
          style={{ maxWidth: 280 }}
          value={provider}
          onChange={(e) => setProvider(e.target.value as CloudProvider)}
          disabled={cloudProviders.length === 0}
        >
          {cloudProviders.map((opt) => (
            <option key={opt.provider_key} value={opt.provider_key}>
              {opt.name}
            </option>
          ))}
        </select>
      </div>

      <label className="consent-check">
        <input
          type="checkbox"
          data-testid="onboarding-consent-check"
          checked={agreed}
          onChange={(e) => setAgreed(e.target.checked)}
          disabled={!bottomReached}
        />
        <span>我已阅读并理解以上内容，同意在云端模式下上传文件内容到第三方服务。</span>
      </label>

      <div className="step-actions step-actions--between">
        <button type="button" className="btn btn--ghost" onClick={onBack}>
          ← 返回选其他模式
        </button>
        <button
          type="button"
          className="btn btn--primary"
          data-testid="onboarding-consent-confirm"
          disabled={!agreed || !bottomReached}
          onClick={() => onConfirm(provider)}
        >
          确认并继续
        </button>
      </div>
    </div>
  );
}
