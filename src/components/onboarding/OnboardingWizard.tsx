// 首次启动引导向导容器（对齐交互原型 §Onboarding）。
//
// 结构：onboarding > onboarding-card > (logo + h1 + subtitle + step-dots) + onboarding-step(s)
// 步骤流转：
//   mode（选模式） → consent（仅云端） → directory（选目录）→ complete
//   选 Local 时跳过 consent 直接进 directory
// 引导不可跳过（设计稿 §3：「引导不可跳过」）。

import { useState } from 'react';
import { useSettingsStore } from '../../stores/settingsStore';
import { useFileStore } from '../../stores/fileStore';
import type { InferenceMode } from '../../types/ipc';
import { StepModeSelect } from './StepModeSelect';
import { StepConsent } from './StepConsent';
import { StepDirectory } from './StepDirectory';

type OnboardingStep = 'mode' | 'consent' | 'directory';

/** SVG 图标（与 Sidebar brand 一致的文件夹图标） */
function FolderIcon() {
  return (
    <svg
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={2}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden
    >
      <path d="M3 7v10a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2V9a2 2 0 0 0-2-2h-6l-2-2H5a2 2 0 0 0-2 2z" />
    </svg>
  );
}

/** 步骤映射到 step-dot 序号（1-based） */
function stepToDot(step: OnboardingStep): number {
  if (step === 'mode') return 1;
  if (step === 'consent') return 2;
  return 3;
}

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
  const handleConsentConfirm = async (provider: string) => {
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

  const currentDot = stepToDot(step);

  return (
    <div className="onboarding" role="dialog" aria-label="首次启动引导">
      <div className="onboarding-card">
        {/* logo + 标题（始终展示） */}
        <div className="logo" aria-hidden>
          <FolderIcon />
        </div>
        <h1>FileMind</h1>
        <p className="subtitle">智能文件整理分类器 + RAG 知识问答系统</p>

        {/* step-dots 进度指示器 */}
        <div className="step-dots" aria-hidden>
          <div className={`step-dot${currentDot >= 1 ? ' active' : ''}`} />
          <div className={`step-dot${currentDot >= 2 ? ' active' : ''}`} />
          <div className={`step-dot${currentDot >= 3 ? ' active' : ''}`} />
        </div>

        {/* 错误提示条 */}
        {error && (
          <div className="onboarding-error" role="alert">
            {error}
            <button
              type="button"
              className="onboarding-error-dismiss"
              onClick={() => setError(null)}
              aria-label="关闭错误"
            >
              ×
            </button>
          </div>
        )}

        {/* 步骤内容（同一时刻只显示一个） */}
        {step === 'mode' && (
          <div className="onboarding-step active">
            <StepModeSelect onNext={handleModeNext} />
          </div>
        )}

        {step === 'consent' && (
          <div className="onboarding-step active">
            <StepConsent onConfirm={handleConsentConfirm} onBack={handleConsentBack} />
          </div>
        )}

        {step === 'directory' && (
          <div className="onboarding-step active">
            <StepDirectory onComplete={handleDirectoryComplete} mode={selectedMode ?? 'Local'} />
          </div>
        )}
      </div>
    </div>
  );
}
