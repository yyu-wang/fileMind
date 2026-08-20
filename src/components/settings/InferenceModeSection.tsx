// 推理模式区块（设置页）：显示当前模式，切换本地/云端。
//
// 安全约束（04 API §2-3）：切回本地经 setInferenceMode（安全阀放行）；
// 切换到云端必须走 signCloudConsent（同意书通道），setInferenceMode 在
// 无同意时会被 Rust 安全阀拒绝，故 UI 层直接走同意书弹窗短路。

import { useState } from 'react';
import { useSettingsStore } from '../../stores/settingsStore';
import type { CloudProvider } from '../../types/ipc';
import { CloudConsentDialog } from './CloudConsentDialog';

export function InferenceModeSection() {
  const inferenceMode = useSettingsStore((s) => s.inferenceMode);
  const setInferenceMode = useSettingsStore((s) => s.setInferenceMode);
  const signCloudConsent = useSettingsStore((s) => s.signCloudConsent);
  const [consentOpen, setConsentOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const isCloud = inferenceMode === 'Cloud';

  const switchToLocal = async () => {
    if (!isCloud) return; // 同模式短路，避免无谓调用
    setBusy(true);
    setError(null);
    try {
      await setInferenceMode('Local');
    } catch (e) {
      setError(e instanceof Error ? e.message : '切换推理模式失败');
    } finally {
      setBusy(false);
    }
  };

  const confirmCloud = async (provider: CloudProvider) => {
    setBusy(true);
    setError(null);
    try {
      await signCloudConsent(provider);
      setConsentOpen(false);
    } catch (e) {
      setError(e instanceof Error ? e.message : '签署同意书失败');
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="settings-section" aria-labelledby="settings-inference-title">
      <h3 id="settings-inference-title" className="settings-section__title">
        推理模式
      </h3>
      <p className="settings-section__desc">
        决定文件数据在哪里被 AI 处理。模式切换会持久化保存，重启后仍然生效。
      </p>
      <div className="settings-row">
        <span className="settings-mode-badge" title="当前推理模式">
          {isCloud ? '☁️ 云端模式' : '🛡️ 本地模式'}
        </span>
        {isCloud ? (
          <button
            type="button"
            className="btn btn--primary"
            onClick={() => void switchToLocal()}
            disabled={busy}
          >
            切回本地
          </button>
        ) : (
          <button
            type="button"
            className="btn btn--ghost"
            onClick={() => setConsentOpen(true)}
            disabled={busy}
          >
            切换到云端
          </button>
        )}
      </div>
      {error && (
        <p className="settings-section__error" role="alert">
          {error}
        </p>
      )}
      {consentOpen && (
        <CloudConsentDialog
          busy={busy}
          onConfirm={(p) => void confirmCloud(p)}
          onCancel={() => setConsentOpen(false)}
        />
      )}
    </section>
  );
}
