// 推理模式切换与云端同意书流程的状态机（设置页 · 推理模式区块）。
//
// 从 InferenceModeSection 抽出——区块此前 156 行，其中一半以上是「签署/撤回 + busy/error
// + 两个弹窗开关」的状态与副作用，与视觉结构无关。
//
// 安全约束（04 API §2-3）：切回本地经 setInferenceMode（安全阀放行）；
// 切换到云端必须走 signCloudConsent（同意书通道），setInferenceMode 在
// 无同意时会被 Rust 安全阀拒绝，故 UI 层直接走同意书弹窗短路。
// T7.5 可撤回：云端模式的「撤回并切回本地」走 revokeCloudConsent（07-§3
// 一键撤回），退出云端即撤回同意，不保留失效同意。

import { useState } from 'react';
import { useSettingsStore } from '@/stores/settingsStore';

/** 推理模式在 UI 层的取值（混合模式 hybrid 为 P1 未开发，不出现）。 */
export type ModeValue = 'local' | 'cloud';

/** 同意书签署/撤回流程的对外出口。 */
export interface CloudConsentFlow {
  /** 签署/撤回请求进行中（按钮防重复提交） */
  busy: boolean;
  /** 流程错误文案（null 表示无错误） */
  error: string | null;
  /** 同意书弹窗是否打开 */
  consentOpen: boolean;
  /** 打开同意书弹窗（点云端选项时） */
  open: () => void;
  /** 关闭同意书弹窗（取消，不签署） */
  close: () => void;
  /** 确认签署同意书并切到云端 */
  confirm: (provider: string) => Promise<void>;
  /** 一键撤回：清除同意并自动切回本地（04 API §2-3d 联动） */
  revoke: () => Promise<void>;
}

/** useInferenceModeSwitch 的对外出口。 */
export interface InferenceModeSwitch {
  /** 当前生效模式 */
  currentMode: ModeValue;
  /** 是否处于云端模式 */
  isCloud: boolean;
  /** 已签署同意的云端提供商 slug（未签署为 null） */
  provider: string | null;
  /** 已签署的同意书版本号（未签署为 null） */
  version: string | null;
  /** 签署时间（ISO 8601，未签署为 null） */
  signedAt: string | null;
  /** 签署/撤回请求进行中 */
  busy: boolean;
  /** 流程错误文案（null 表示无错误） */
  error: string | null;
  /** 同意书弹窗是否打开 */
  consentOpen: boolean;
  /** 「查看知情同意书」弹窗是否打开 */
  viewConsentOpen: boolean;
  /** 点击 mode-option（切换语义见实现注释） */
  requestMode: (value: ModeValue) => void;
  /** 确认签署同意书 */
  confirmCloud: (provider: string) => Promise<void>;
  /** 一键撤回并切回本地 */
  revoke: () => Promise<void>;
  /** 打开「查看知情同意书」弹窗 */
  openConsentView: () => void;
  /** 关闭同意书弹窗 */
  closeConsent: () => void;
  /** 关闭「查看知情同意书」弹窗 */
  closeConsentView: () => void;
}

/** 统一错误文案：Error 用 message，其余用兜底文案。 */
function messageOf(err: unknown, fallback: string): string {
  return err instanceof Error ? err.message : fallback;
}

/**
 * 云端同意书的签署/撤回流程（busy/error + 弹窗开关）。
 *
 * Returns:
 *   弹窗开关、busy/error 与签署/撤回动作（见 CloudConsentFlow）
 */
export function useCloudConsentFlow(): CloudConsentFlow {
  const signCloudConsent = useSettingsStore((s) => s.signCloudConsent);
  const revokeCloudConsent = useSettingsStore((s) => s.revokeCloudConsent);
  const [consentOpen, setConsentOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  /** 一键撤回：清除同意并自动切回本地（04 API §2-3d 联动）。 */
  const revoke = async (): Promise<void> => {
    setBusy(true);
    setError(null);
    try {
      await revokeCloudConsent();
    } catch (err) {
      setError(messageOf(err, '撤回同意失败'));
    } finally {
      setBusy(false);
    }
  };

  /** 确认签署（无论成败都收尾 busy 并关闭弹窗，失败文案经 error 横幅呈现）。 */
  const confirm = async (provider: string): Promise<void> => {
    setBusy(true);
    setError(null);
    try {
      await signCloudConsent(provider);
    } catch (err) {
      setError(messageOf(err, '签署同意书失败'));
    } finally {
      setBusy(false);
      setConsentOpen(false);
    }
  };

  return {
    busy,
    error,
    consentOpen,
    open: () => setConsentOpen(true),
    close: () => setConsentOpen(false),
    confirm,
    revoke,
  };
}

/**
 * 推理模式切换：模式映射 + 点击语义 + 同意书流程。
 *
 * Returns:
 *   模式、同意书元数据、弹窗开关与三个交互入口（见 InferenceModeSwitch）
 */
export function useInferenceModeSwitch(): InferenceModeSwitch {
  const inferenceMode = useSettingsStore((s) => s.inferenceMode);
  const provider = useSettingsStore((s) => s.cloudConsentProvider);
  const version = useSettingsStore((s) => s.cloudConsentVersion);
  const signedAt = useSettingsStore((s) => s.cloudConsentSignedAt);
  const consent = useCloudConsentFlow();
  const [viewConsentOpen, setViewConsentOpen] = useState(false);

  const isCloud = inferenceMode === 'Cloud';
  const currentMode: ModeValue = isCloud ? 'cloud' : 'local';

  /**
   * 点击 mode-option 的语义：
   * - 点 local：当前已是 local 则 noop；当前 cloud 则触发撤回流程
   * - 点 cloud：当前已是 cloud 则 noop；当前 local 则触发同意书弹窗
   */
  const requestMode = (value: ModeValue) => {
    if (value === currentMode) return;
    if (value === 'cloud') {
      consent.open();
    } else {
      void consent.revoke();
    }
  };

  return {
    currentMode,
    isCloud,
    provider,
    version,
    signedAt,
    busy: consent.busy,
    error: consent.error,
    consentOpen: consent.consentOpen,
    viewConsentOpen,
    requestMode,
    confirmCloud: consent.confirm,
    revoke: consent.revoke,
    openConsentView: () => setViewConsentOpen(true),
    closeConsent: consent.close,
    closeConsentView: () => setViewConsentOpen(false),
  };
}
