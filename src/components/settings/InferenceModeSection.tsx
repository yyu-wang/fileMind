// 推理模式区块（设置页 · 对齐交互原型 §设置 §推理模式）：
// mode-selector(local/cloud/hybrid) + radio-dot 选中态。
//
// 安全约束（04 API §2-3）：切回本地经 setInferenceMode（安全阀放行）；
// 切换到云端必须走 signCloudConsent（同意书通道），setInferenceMode 在
// 无同意时会被 Rust 安全阀拒绝，故 UI 层直接走同意书弹窗短路。
// T7.5 可撤回：云端模式的「撤回并切回本地」走 revokeCloudConsent（07-§3
// 一键撤回），退出云端即撤回同意，不保留失效同意。
//
// 文档结构：
//   <div class="mode-selector" role="radiogroup">
//     <div class="mode-option local selected" role="radio" aria-checked="true">
//       <div class="mode-icon">🛡️</div>
//       <div class="mode-info"><div class="name">本地模式（当前）</div>...</div>
//       <div class="radio-dot"></div>
//     </div>
//     <div class="mode-option cloud">...</div>
//     <div class="mode-option hybrid">...</div>
//   </div>
//   <div class="settings-row settings-row--actions">
//     <button>查看知情同意书</button>
//     <button>撤回云端同意</button>  // 仅云端模式可见
//   </div>

import { useState } from 'react';
import { useSettingsStore } from '../../stores/settingsStore';
import type { CloudProvider } from '../../types/ipc';
import { CloudConsentDialog } from './CloudConsentDialog';
import { ConsentViewDialog } from './ConsentViewDialog';

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

type ModeValue = 'local' | 'cloud' | 'hybrid';

interface ModeOption {
  value: ModeValue;
  icon: string;
  name: string;
  desc: string;
  /** 是否可点击切换（hybrid 暂未支持，仅展示） */
  clickable: boolean;
}

const MODE_OPTIONS: ModeOption[] = [
  {
    value: 'local',
    icon: '🛡️',
    name: '本地模式',
    desc: 'Ollama 本地推理 · 数据不离开设备',
    clickable: true,
  },
  {
    value: 'cloud',
    icon: '☁️',
    name: '云端模式',
    desc: 'API 推理 · 文件内容上传第三方',
    clickable: true,
  },
  {
    value: 'hybrid',
    icon: '🔀',
    name: '混合模式',
    desc: '按功能配置推理位置 · P1 不支持，敬请期待',
    clickable: false,
  },
];

export function InferenceModeSection() {
  const inferenceMode = useSettingsStore((s) => s.inferenceMode);
  const signCloudConsent = useSettingsStore((s) => s.signCloudConsent);
  const revokeCloudConsent = useSettingsStore((s) => s.revokeCloudConsent);
  const cloudConsentProvider = useSettingsStore((s) => s.cloudConsentProvider);
  const cloudConsentVersion = useSettingsStore((s) => s.cloudConsentVersion);
  const cloudConsentSignedAt = useSettingsStore((s) => s.cloudConsentSignedAt);
  const [consentOpen, setConsentOpen] = useState(false);
  const [viewConsentOpen, setViewConsentOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const isCloud = inferenceMode === 'Cloud';
  const currentMode: ModeValue = isCloud ? 'cloud' : 'local';

  /** 一键撤回：清除同意并自动切回本地（04 API §2-3d 联动）。 */
  const revokeConsent = async () => {
    setBusy(true);
    setError(null);
    try {
      await revokeCloudConsent();
    } catch (e) {
      setError(e instanceof Error ? e.message : '撤回同意失败');
    } finally {
      setBusy(false);
    }
  };

  /**
   * 点击 mode-option 的语义：
   * - 点 local：当前已是 local 则 noop；当前 cloud 则触发撤回流程
   * - 点 cloud：当前已是 cloud 则 noop；当前 local 则触发同意书弹窗
   * - 点 hybrid：P1 不支持，noop
   */
  const handleModeClick = (value: ModeValue) => {
    if (value === currentMode) return;
    if (value === 'cloud') {
      setConsentOpen(true);
    } else if (value === 'local' && currentMode === 'cloud') {
      void revokeConsent();
    }
  };

  const confirmCloud = async (provider: CloudProvider) => {
    setBusy(true);
    setError(null);
    try {
      await signCloudConsent(provider);
    } catch (e) {
      setError(e instanceof Error ? e.message : '签署同意书失败');
    } finally {
      setBusy(false);
      setConsentOpen(false);
    }
  };

  return (
    <section className="settings-section" aria-labelledby="settings-inference-title">
      <h3 id="settings-inference-title" className="settings-section__title">
        🛡️ 推理模式
      </h3>
      <p className="section-desc">
        决定文件数据在哪里被 AI 处理。模式切换会持久化保存，重启后仍然生效。
      </p>

      <div className="mode-selector" role="radiogroup" aria-label="推理模式">
        {MODE_OPTIONS.map((opt) => {
          const isSelected = opt.value === currentMode;
          // 已签署云端同意 → 云端 mode-option desc 后追加「已签署同意」tag
          const showSignedTag = opt.value === 'cloud' && isCloud && cloudConsentProvider;
          return (
            <div
              key={opt.value}
              className={`mode-option ${opt.value}${isSelected ? ' selected' : ''}`}
              role="radio"
              aria-checked={isSelected}
              aria-label={`${opt.icon} ${opt.name}${isSelected ? '（当前）' : ''}`}
              data-testid={`mode-option-${opt.value}`}
              tabIndex={opt.clickable ? 0 : -1}
              onClick={() => opt.clickable && handleModeClick(opt.value)}
              onKeyDown={(e) => {
                if (opt.clickable && (e.key === 'Enter' || e.key === ' ')) {
                  e.preventDefault();
                  handleModeClick(opt.value);
                }
              }}
            >
              <div className="mode-icon" aria-hidden>
                {opt.icon}
              </div>
              <div className="mode-info">
                <div className="name">
                  {opt.name}
                  {isSelected && '（当前）'}
                </div>
                <div className="desc">
                  {opt.desc}
                  {showSignedTag && (
                    <>
                      {' · '}
                      <span className="tag tag-green" style={{ fontSize: 10 }}>
                        已签署同意
                      </span>
                    </>
                  )}
                </div>
              </div>
              <div className="radio-dot" aria-hidden />
            </div>
          );
        })}
      </div>

      {/* 原型 05_交互原型 §设置页推理模式区：底部并列「查看知情同意书」+「撤回云端同意」 */}
      <div className="settings-row settings-row--actions">
        <button
          type="button"
          className="btn btn--ghost btn--sm"
          onClick={() => setViewConsentOpen(true)}
        >
          查看知情同意书
        </button>
        {isCloud && (
          <button
            type="button"
            className="btn btn--ghost btn--sm"
            style={{ color: 'var(--warn)' }}
            onClick={() => void revokeConsent()}
            disabled={busy}
          >
            撤回并切回本地
          </button>
        )}
      </div>

      {isCloud && cloudConsentProvider && (
        <p className="settings-section__desc settings-section__consent-info">
          已签署同意书 · {cloudConsentProvider} · 版本 {cloudConsentVersion ?? '未知'} ·
          {cloudConsentSignedAt ? ` ${formatSignedAt(cloudConsentSignedAt)}` : ''}
        </p>
      )}
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
      {viewConsentOpen && <ConsentViewDialog onClose={() => setViewConsentOpen(false)} />}
    </section>
  );
}
