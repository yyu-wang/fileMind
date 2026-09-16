// 推理模式区块（设置页 · 对齐交互原型 §设置 §推理模式）。
//
// 本文件只负责区块外壳：标题/说明、两个视觉区域（InferenceModeOptions /
// InferenceModeActions）的拼装、签署信息与错误提示、两个弹窗挂载；
// 状态与副作用收在 useInferenceModeSwitch，选项区与操作行各自成组件。
//
// 安全约束（04 API §2-3）：切回本地经 setInferenceMode（安全阀放行）；
// 切换到云端必须走 signCloudConsent（同意书通道），setInferenceMode 在
// 无同意时会被 Rust 安全阀拒绝，故 UI 层直接走同意书弹窗短路。
// T7.5 可撤回：云端模式的「撤回并切回本地」走 revokeCloudConsent（07-§3
// 一键撤回），退出云端即撤回同意，不保留失效同意。

import { CloudConsentDialog } from './CloudConsentDialog';
import { ConsentViewDialog } from './ConsentViewDialog';
import { InferenceModeActions } from './InferenceModeActions';
import { InferenceModeOptions } from './InferenceModeOptions';
import { useInferenceModeSwitch } from './useInferenceModeSwitch';

/** 日期展示格式：YYYY-MM-DD HH:mm（zh-CN 时区本地时间）。 */
function formatSignedAt(iso: string): string {
  return new Date(iso).toLocaleString('zh-CN', {
    year: 'numeric',
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
  });
}

export function InferenceModeSection() {
  const mode = useInferenceModeSwitch();

  return (
    <section className="settings-section" aria-labelledby="settings-inference-title">
      <h3 id="settings-inference-title" className="settings-section__title">
        🛡️ 推理模式
      </h3>
      <p className="section-desc">
        决定文件数据在哪里被 AI 处理。模式切换会持久化保存，重启后仍然生效。
      </p>

      <InferenceModeOptions
        current={mode.currentMode}
        cloudSigned={mode.isCloud && Boolean(mode.provider)}
        onSelect={mode.requestMode}
      />

      <InferenceModeActions
        isCloud={mode.isCloud}
        busy={mode.busy}
        onViewConsent={mode.openConsentView}
        onRevoke={mode.revoke}
      />

      {mode.isCloud && mode.provider && (
        <p className="settings-section__desc settings-section__consent-info">
          已签署同意书 · {mode.provider} · 版本 {mode.version ?? '未知'} ·
          {mode.signedAt ? ` ${formatSignedAt(mode.signedAt)}` : ''}
        </p>
      )}
      {mode.error && (
        <p className="settings-section__error" role="alert">
          {mode.error}
        </p>
      )}
      {mode.consentOpen && (
        <CloudConsentDialog
          busy={mode.busy}
          onConfirm={(p) => void mode.confirmCloud(p)}
          onCancel={mode.closeConsent}
        />
      )}
      {mode.viewConsentOpen && <ConsentViewDialog onClose={mode.closeConsentView} />}
    </section>
  );
}
