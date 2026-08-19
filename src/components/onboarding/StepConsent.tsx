// 步骤二：云端知情同意书（03 设计稿 §3.2，07 规范 §隐私合规）。
//
// 仅云端模式显示，勾选 + 确认后调用 signCloudConsent。
// 未勾选时「确认」按钮 disabled。

import { useState } from 'react';
import type { CloudProvider } from '../../types/ipc';

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
  const [provider, setProvider] = useState<CloudProvider>('Openai');

  return (
    <div className="onboarding__step">
      <h2 className="onboarding__title">隐私知情同意书</h2>

      <div className="onboarding__hint onboarding__hint--warn">
        <div className="onboarding__hint-title">⚠️ 请仔细阅读以下内容</div>
        <div className="onboarding__hint-body">
          <p>
            <strong>选择云端模式意味着：</strong>
          </p>
          <ul>
            <li>
              你的<strong>文件内容</strong>将被发送到第三方 AI 服务提供商（如 OpenAI、DeepSeek）
            </li>
            <li>这些内容将在对方服务器上处理以生成 AI 回答</li>
            <li>虽然提供商有保密政策，但数据已离开你的设备</li>
          </ul>
          <p>
            <strong>你可以随时撤回同意：</strong>
            在设置中撤回后，应用自动切换回本地模式，不会再上传任何数据。
          </p>
        </div>
      </div>

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
          disabled={!agreed}
          onClick={() => onConfirm(provider)}
        >
          确认并继续
        </button>
      </div>
    </div>
  );
}
