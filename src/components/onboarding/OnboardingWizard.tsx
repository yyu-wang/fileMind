// 首次启动引导向导容器（03 设计稿 §3 首次启动引导）。
//
// 步骤流转：
//   mode（选模式） → consent（仅云端） → directory（选目录）→ complete
//   选 Local 时跳过 consent 直接进 directory
//
// 引导不可跳过（设计稿 §3：「引导不可跳过」）。

import { useState } from 'react';
import { useSettingsStore } from '../../stores/settingsStore';
import { useFileStore } from '../../stores/fileStore';
import type { CloudProvider, InferenceMode } from '../../types/ipc';
import { StepModeSelect } from './StepModeSelect';
import { StepConsent } from './StepConsent';
import { StepDirectory } from './StepDirectory';

type OnboardingStep = 'mode' | 'consent' | 'directory';

export function OnboardingWizard() {
  const [step, setStep] = useState<OnboardingStep>('mode');
  const [selectedMode, setSelectedMode] = useState<InferenceMode | null>(null);
  const [error, setError] = useState<string | null>(null);

  const { signCloudConsent, completeOnboarding, setInferenceMode } = useSettingsStore();
  const scanFiles = useFileStore((s) => s.scanFiles);

  /** 步骤一选模式后的下一步 */
  const handleModeNext = (mode: InferenceMode) => {
    setSelectedMode(mode);
    if (mode === 'Cloud') {
      setStep('consent');
    } else {
      // Local 模式跳过 consent 直接进 directory
      void handleModeSelected(mode);
    }
  };

  /** Local 模式直接调用 setInferenceMode 后进入 directory */
  const handleModeSelected = async (mode: InferenceMode) => {
    try {
      await setInferenceMode(mode);
      setStep('directory');
    } catch (e) {
      setError(e instanceof Error ? e.message : '推理模式切换失败');
    }
  };

  /** 步骤二同意书确认 */
  const handleConsentConfirm = async (provider: CloudProvider) => {
    try {
      await signCloudConsent(provider);
      setStep('directory');
    } catch (e) {
      setError(e instanceof Error ? e.message : '同意书签署失败');
    }
  };

  /** 步骤二返回 → 重新选模式 */
  const handleConsentBack = () => {
    setStep('mode');
  };

  /** 步骤三选目录并完成 */
  const handleDirectoryComplete = async (path: string) => {
    try {
      await completeOnboarding(path);
      // 引导完成后触发首次扫描
      await scanFiles(path);
    } catch (e) {
      setError(e instanceof Error ? e.message : '完成引导失败');
    }
  };

  return (
    <div className="onboarding" role="dialog" aria-label="首次启动引导">
      <div className="onboarding__container">
        {error && (
          <div className="onboarding__error" role="alert">
            {error}
            <button
              type="button"
              className="onboarding__error-dismiss"
              onClick={() => setError(null)}
              aria-label="关闭错误"
            >
              ×
            </button>
          </div>
        )}

        {step === 'mode' && <StepModeSelect onNext={handleModeNext} />}

        {step === 'consent' && (
          <StepConsent onConfirm={handleConsentConfirm} onBack={handleConsentBack} />
        )}

        {step === 'directory' && (
          <StepDirectory onComplete={handleDirectoryComplete} mode={selectedMode ?? 'Local'} />
        )}
      </div>
    </div>
  );
}
