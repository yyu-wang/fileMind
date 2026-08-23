// 步骤二：云端知情同意书（03 设计稿 §3.2，07 规范 §隐私合规）。
//
// 仅云端模式显示，勾选 + 确认后调用 signCloudConsent。
// T7.5 滚动到底门控：未滚读完同意书前 checkbox 禁用，勾选后才能确认。

import { useState } from 'react';
import { CLOUD_CONSENT_VERSION } from '../../lib/consent';
import type { CloudProvider } from '../../types/ipc';
import { ConsentAgreement } from '../consent/ConsentAgreement';

interface StepConsentProps {
  onConfirm: (provider: CloudProvider) => void;
  onBack: () => void;
}

const PROVIDER_OPTIONS: Array<{ value: CloudProvider; label: string }> = [
  { value: 'Openai', label: 'OpenAI（gpt-4o 等）' },
  { value: 'Deepseek', label: 'DeepSeek' },
];

export function StepConsent({ onConfirm, onBack }: StepConsentProps) {
  const [agreed, setAgreed] = useState(false);
  const [bottomReached, setBottomReached] = useState(false);
  const [provider, setProvider] = useState<CloudProvider>('Openai');

  return (
    <div className="onboarding__step">
      <h2 className="onboarding__title">隐私知情同意书</h2>

      <ConsentAgreement version={CLOUD_CONSENT_VERSION} onBottomReached={setBottomReached} />

      <div className="onboarding__provider-select">
        <label className="onboarding__field-label">选择云端提供商：</label>
        <select
          value={provider}
          onChange={(e) => setProvider(e.target.value as CloudProvider)}
          className="onboarding__select"
        >
          {PROVIDER_OPTIONS.map((opt) => (
            <option key={opt.value} value={opt.value}>
              {opt.label}
            </option>
          ))}
        </select>
      </div>

      <label className="onboarding__consent-check">
        <input
          type="checkbox"
          checked={agreed}
          onChange={(e) => setAgreed(e.target.checked)}
          className="onboarding__checkbox-input"
          disabled={!bottomReached}
        />
        <span>我已阅读并理解以上内容，同意在云端模式下上传文件内容到第三方服务。</span>
      </label>

      <div className="onboarding__actions onboarding__actions--between">
        <button type="button" className="btn btn--ghost" onClick={onBack}>
          ← 返回选其他模式
        </button>
        <button
          type="button"
          className="btn btn--primary"
          disabled={!agreed || !bottomReached}
          onClick={() => onConfirm(provider)}
        >
          确认并继续
        </button>
      </div>
    </div>
  );
}
