// 云端知情同意书对话框（设置页切云端用，内容对齐 StepConsent / 07 规范 §隐私合规）。
//
// 作为 modal 复用同意书文案与提供商选择；确认回调由父组件调 signCloudConsent。
// T7.5 滚动到底门控：未滚读完同意书前 checkbox 禁用，勾选后才能确认。

import { useState } from 'react';
import { CLOUD_CONSENT_VERSION } from '../../lib/consent';
import type { CloudProvider } from '../../types/ipc';
import { ConsentAgreement } from '../consent/ConsentAgreement';

interface CloudConsentDialogProps {
  busy: boolean;
  onConfirm: (provider: CloudProvider) => void;
  onCancel: () => void;
}

const PROVIDER_OPTIONS: Array<{ value: CloudProvider; label: string }> = [
  { value: 'Openai', label: 'OpenAI（gpt-4o 等）' },
  { value: 'Deepseek', label: 'DeepSeek' },
];

export function CloudConsentDialog({ busy, onConfirm, onCancel }: CloudConsentDialogProps) {
  const [agreed, setAgreed] = useState(false);
  const [bottomReached, setBottomReached] = useState(false);
  const [provider, setProvider] = useState<CloudProvider>('Openai');

  return (
    <div
      className="settings-dialog__backdrop"
      role="presentation"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget && !busy) onCancel();
      }}
    >
      <div className="settings-dialog" role="dialog" aria-modal="true" aria-label="隐私知情同意书">
        <h3 className="settings-dialog__title">隐私知情同意书</h3>

        <ConsentAgreement version={CLOUD_CONSENT_VERSION} onBottomReached={setBottomReached} />

        <div className="onboarding__provider-select">
          <label className="onboarding__field-label">选择云端提供商：</label>
          <select
            value={provider}
            onChange={(e) => setProvider(e.target.value as CloudProvider)}
            className="onboarding__select"
            disabled={busy}
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
            disabled={!bottomReached || busy}
          />
          <span>我已阅读并理解以上内容，同意在云端模式下上传文件内容到第三方服务。</span>
        </label>

        <div className="settings-dialog__actions">
          <button type="button" className="btn btn--ghost" onClick={onCancel} disabled={busy}>
            取消
          </button>
          <button
            type="button"
            className="btn btn--primary"
            disabled={!agreed || !bottomReached || busy}
            onClick={() => onConfirm(provider)}
          >
            确认并切换到云端
          </button>
        </div>
      </div>
    </div>
  );
}
