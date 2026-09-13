// 同意书只读查看弹窗（设置页「查看知情同意书」入口）。
//
// 与 CloudConsentDialog 的差异：无提供商选择 / 无勾选确认 / 无切换动作，
// 仅展示同意书全文 + 关闭按钮，供已签署用户随时回看同意内容。
// 复用 ConsentAgreement 承载滚动内容；onBottomReached 此处无业务含义，
// 传 noop 避免影响（ConsentAgreement 内部仍做滚动检测，不影响渲染）。

import { CLOUD_CONSENT_VERSION } from '../../lib/consent';
import { ConsentAgreement } from '../consent/ConsentAgreement';

interface ConsentViewDialogProps {
  /** 关闭回调。 */
  onClose: () => void;
}

/** 空回调：ConsentAgreement 必填 onBottomReached，只读查看模式无业务含义。 */
const noopBottomReached = () => {};

export function ConsentViewDialog({ onClose }: ConsentViewDialogProps) {
  return (
    <div
      className="settings-dialog__backdrop"
      role="presentation"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div className="settings-dialog" role="dialog" aria-modal="true" aria-label="隐私知情同意书">
        <h3 className="settings-dialog__title">隐私知情同意书</h3>
        <ConsentAgreement version={CLOUD_CONSENT_VERSION} onBottomReached={noopBottomReached} />
        <div className="settings-dialog__actions">
          <button type="button" className="btn btn--ghost" onClick={onClose}>
            关闭
          </button>
        </div>
      </div>
    </div>
  );
}
