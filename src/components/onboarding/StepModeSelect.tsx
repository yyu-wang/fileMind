// 步骤一：推理模式选择（对齐交互原型 §Onboarding Step 1）。
//
// 三个选项：Local（推荐）/ Cloud / Hybrid（禁用，T11）
// 选 Local → 直接进 directory；选 Cloud → 进 consent
// Ollama 检测：挂载时 probeOllama，callout 反映真实状态。

import { useEffect, useState } from 'react';
import { useSettingsStore } from '../../stores/settingsStore';
import type { InferenceMode } from '../../types/ipc';

interface StepModeSelectProps {
  onNext: (mode: InferenceMode) => void;
}

type ModeOptionValue = InferenceMode | 'Hybrid';

interface ModeOption {
  value: ModeOptionValue;
  icon: string;
  label: string;
  desc: string;
  tag?: string;
  tagClass?: string;
  disabled?: boolean;
}

const MODE_OPTIONS: ModeOption[] = [
  {
    value: 'Local',
    icon: '🛡️',
    label: '本地模式',
    desc: '所有 AI 处理在你的设备本地完成，数据不离开设备。需要 Ollama 运行。',
    tag: '推荐',
    tagClass: 'tag-purple',
  },
  {
    value: 'Cloud',
    icon: '☁️',
    label: '云端模式',
    desc: '使用 OpenAI/DeepSeek 云端 API。效果更强但文件内容会上传到第三方服务器。',
  },
  {
    value: 'Hybrid',
    icon: '🔀',
    label: '混合模式',
    desc: '按功能粒度配置：分类用本地、问答用云端等。适合高级用户。',
    tag: '即将推出',
    tagClass: 'tag-gray',
    disabled: true,
  },
];

export function StepModeSelect({ onNext }: StepModeSelectProps) {
  const [selected, setSelected] = useState<ModeOptionValue>('Local');
  const ollamaStatus = useSettingsStore((s) => s.ollamaStatus);
  const ollamaProbing = useSettingsStore((s) => s.ollamaProbing);
  const probeOllama = useSettingsStore((s) => s.probeOllama);

  useEffect(() => {
    void probeOllama();
  }, [probeOllama]);

  const ollamaCallout = ollamaProbing ? (
    <div className="callout info">
      <span style={{ color: 'var(--accent2)', fontWeight: 500 }}>⏳ 正在检测本地 Ollama...</span>
    </div>
  ) : ollamaStatus?.available ? (
    <div className="callout success">
      <span style={{ color: 'var(--success)', fontWeight: 500 }}>
        ✓ 已检测到 Ollama · 推荐本地模式
      </span>
    </div>
  ) : ollamaStatus ? (
    <div className="callout warn">
      <span style={{ color: 'var(--warn)', fontWeight: 500 }}>
        ⚠️ 未检测到本地 Ollama。本地模式需要先安装并启动 Ollama。
      </span>
    </div>
  ) : (
    <div className="callout warn">
      <span style={{ color: 'var(--warn)', fontWeight: 500 }}>
        ⚠️ 暂时无法检测 Ollama，请稍后重试。
      </span>
    </div>
  );

  return (
    <div>
      <h3>选择你的 AI 推理模式</h3>
      <p className="step-desc">这决定你的文件数据在哪里被 AI 处理。你可以随时在设置中更改。</p>

      {ollamaCallout}

      <div className="mode-selector" role="radiogroup" aria-label="推理模式选择">
        {MODE_OPTIONS.map((opt) => {
          const isSelected = selected === opt.value;
          return (
            <div
              key={opt.value}
              className={`mode-option ${opt.value.toLowerCase()}${isSelected ? ' selected' : ''}${opt.disabled ? ' disabled' : ''}`}
              role="radio"
              aria-checked={isSelected}
              aria-label={`${opt.label}${opt.disabled ? '（即将推出）' : ''}`}
              tabIndex={opt.disabled ? -1 : 0}
              onClick={() => !opt.disabled && setSelected(opt.value)}
              onKeyDown={(e) => {
                if (!opt.disabled && (e.key === 'Enter' || e.key === ' ')) {
                  e.preventDefault();
                  setSelected(opt.value);
                }
              }}
              style={opt.disabled ? { opacity: 0.5, cursor: 'not-allowed' } : undefined}
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
          onClick={() => {
            if (selected === 'Local' || selected === 'Cloud') {
              onNext(selected);
            }
          }}
          disabled={selected === 'Hybrid'}
        >
          下一步 →
        </button>
      </div>
    </div>
  );
}
