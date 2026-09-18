// 步骤一：推理模式选择（对齐交互原型 §Onboarding Step 1）。
//
// 两个选项：Local（推荐）/ Cloud。混合模式（Hybrid）P1 未开发，不渲染占位选项。
// 选 Local → 直接进 directory；选 Cloud → 进 consent
// Ollama 检测：挂载时 probeOllama，callout 反映真实状态。

import { useEffect, useState } from 'react';
import { useSettingsStore } from '../../stores/settingsStore';
import type { InferenceMode, OllamaStatus } from '../../types/ipc';

interface StepModeSelectProps {
  onNext: (mode: InferenceMode) => void;
}

interface ModeOption {
  value: InferenceMode;
  icon: string;
  label: string;
  desc: string;
  tag?: string;
  tagClass?: string;
}

const MODE_OPTIONS: ModeOption[] = [
  {
    value: 'Local',
    icon: '🛡️',
    label: '本地模式',
    desc: '所有 AI 处理在你的设备本地完成，数据不离开设备。使用内置引擎或本机 Ollama。',
    tag: '推荐',
    tagClass: 'tag-purple',
  },
  {
    value: 'Cloud',
    icon: '☁️',
    label: '云端模式',
    desc: '使用 OpenAI/DeepSeek 云端 API。效果更强但文件内容会上传到第三方服务器。',
  },
];

interface OllamaCalloutProps {
  probing: boolean;
  status: OllamaStatus | null;
}

/**
 * Ollama 检测提示条：检测中 / 已检测到 / 未检测到 / 检测失败。
 *
 * 「未检测到」不等于本地模式不可用：内置 llama.cpp 引擎（T3）会接管本地生成，
 * 故这里给的是「下一步怎么做」，而不是「必须先装 Ollama」。
 */
function OllamaCallout({ probing, status }: OllamaCalloutProps) {
  if (probing) {
    return (
      <div className="callout info">
        <span style={{ color: 'var(--accent2)', fontWeight: 500 }}>⏳ 正在检测本地 Ollama...</span>
      </div>
    );
  }
  if (status?.available) {
    return (
      <div className="callout success">
        <span style={{ color: 'var(--success)', fontWeight: 500 }}>
          ✓ 已检测到 Ollama · 推荐本地模式
        </span>
      </div>
    );
  }
  if (!status) {
    return (
      <div className="callout warn">
        <span style={{ color: 'var(--warn)', fontWeight: 500 }}>
          ⚠️ 暂时无法检测 Ollama，请稍后重试。
        </span>
      </div>
    );
  }
  return (
    <div className="callout warn">
      <span style={{ color: 'var(--warn)', fontWeight: 500 }}>
        ⚠️ 未检测到本地 Ollama。已下载内置生成模型（设置 →
        本地生成模型）时，本地模式会自动改用内置引擎，无需安装 Ollama。
      </span>
    </div>
  );
}

export function StepModeSelect({ onNext }: StepModeSelectProps) {
  const [selected, setSelected] = useState<InferenceMode>('Local');
  const ollamaStatus = useSettingsStore((s) => s.ollamaStatus);
  const ollamaProbing = useSettingsStore((s) => s.ollamaProbing);
  const probeOllama = useSettingsStore((s) => s.probeOllama);

  useEffect(() => {
    void probeOllama();
  }, [probeOllama]);

  return (
    <div>
      <h3>选择你的 AI 推理模式</h3>
      <p className="step-desc">这决定你的文件数据在哪里被 AI 处理。你可以随时在设置中更改。</p>

      <OllamaCallout probing={ollamaProbing} status={ollamaStatus} />

      <div className="mode-selector" role="radiogroup" aria-label="推理模式选择">
        {MODE_OPTIONS.map((opt) => {
          const isSelected = selected === opt.value;
          return (
            <div
              key={opt.value}
              className={`mode-option ${opt.value.toLowerCase()}${isSelected ? ' selected' : ''}`}
              role="radio"
              aria-checked={isSelected}
              aria-label={opt.label}
              tabIndex={0}
              onClick={() => setSelected(opt.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter' || e.key === ' ') {
                  e.preventDefault();
                  setSelected(opt.value);
                }
              }}
            >
              <div className="mode-icon" aria-hidden>
                {opt.icon}
              </div>
              <div className="mode-info">
                <div className="name">
                  {opt.label}
                  {opt.tag && (
                    <span
                      className={`tag ${opt.tagClass ?? ''}`}
                      style={{ marginLeft: 4, fontSize: 10 }}
                    >
                      {opt.tag}
                    </span>
                  )}
                </div>
                <div className="desc">{opt.desc}</div>
              </div>
              <div className="radio-dot" aria-hidden />
            </div>
          );
        })}
      </div>

      <div className="step-actions step-actions--end">
        <button
          type="button"
          className="btn btn--primary"
          data-testid="onboarding-next"
          onClick={() => onNext(selected)}
        >
          下一步 →
        </button>
      </div>
    </div>
  );
}
