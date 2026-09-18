// 推理模式区块底部操作行（原型 05_交互原型 §设置页推理模式区）：
// 并列「查看知情同意书」+「撤回并切回本地」，后者仅云端模式可见。
//
// 与 InferenceModeSection 分离的原因：这是独立视觉区域，可见性与禁用态只取决于
// 云端态与 busy，不参与区块内的同意书弹窗流程。

export interface InferenceModeActionsProps {
  /** 是否处于云端模式（决定「撤回并切回本地」是否渲染） */
  isCloud: boolean;
  /** 撤回请求进行中（按钮禁用防重复提交） */
  busy: boolean;
  /** 打开「查看知情同意书」弹窗 */
  onViewConsent: () => void;
  /** 一键撤回：清除同意并自动切回本地（04 API §2-3d 联动） */
  onRevoke: () => Promise<void>;
}

export function InferenceModeActions({
  isCloud,
  busy,
  onViewConsent,
  onRevoke,
}: InferenceModeActionsProps) {
  return (
    <div className="settings-row settings-row--actions">
      <button type="button" className="btn btn--ghost btn--sm" onClick={onViewConsent}>
        查看知情同意书
      </button>
      {isCloud && (
        <button
          type="button"
          className="btn btn--ghost btn--sm"
          style={{ color: 'var(--warn)' }}
          onClick={() => void onRevoke()}
          disabled={busy}
        >
          撤回并切回本地
        </button>
      )}
    </div>
  );
}
