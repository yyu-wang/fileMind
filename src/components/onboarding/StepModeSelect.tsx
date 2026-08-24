// 步骤一：推理模式选择（03 设计稿 §3.1）。
//
// 三个选项：Local（推荐）/ Cloud / Hybrid（禁用，T11）
// 选 Local → 直接进 directory；选 Cloud → 进 consent
//
// Ollama 检测占位 → 真实探测（T6.7）：挂载时 probeOllama，提示条反映真实状态。

import { useEffect, useState } from 'react';
import { useSettingsStore } from '../../stores/settingsStore';
import type { InferenceMode } from '../../types/ipc';

interface StepModeSelectProps {
  onNext: (mode: InferenceMode) => void;
}

// 选项值用 string 表示（Hybrid 不在 InferenceMode 枚举中，仅作 UI 占位）
type ModeOptionValue = InferenceMode | 'Hybrid';

interface ModeOption {
  value: ModeOptionValue;
  icon: string;
  label: string;
  desc: string;
  tag?: string;
  tagColor?: string;
  disabled?: boolean;
}

const MODE_OPTIONS: ModeOption[] = [
  {
    value: 'Local',
    icon: '🛡️',
    label: '本地模式',
    desc: '所有 AI 处理在你的设备本地完成，数据不离开设备。需要 Ollama 运行。',
    tag: '推荐',
    tagColor: '#e9d8fd',
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
    tagColor: '#fed7aa',
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

  const ollamaHint = ollamaProbing ? (
    <div className="onboarding__hint">⏳ 正在检测本地 Ollama...</div>
  ) : ollamaStatus?.available ? (
    <div className="onboarding__hint onboarding__hint--success">
      ✓ 已检测到 Ollama · 推荐本地模式
    </div>
  ) : ollamaStatus ? (
    <div className="onboarding__hint onboarding__hint--warn">
      ⚠️ 未检测到本地 Ollama。本地模式需要先安装并启动 Ollama。
    </div>
  ) : (
    <div className="onboarding__hint onboarding__hint--warn">
      ⚠️ 暂时无法检测 Ollama，请稍后重试。
    </div>
  );

  return (
    <div className="onboarding__step">
      <h2 className="onboarding__title">选择你的 AI 推理模式</h2>
      <p className="onboarding__subtitle">
        这决定你的文件数据在哪里被 AI 处理。你可以随时在设置中更改。
      </p>

      {ollamaHint}

      <div className="onboarding__options" role="radiogroup" aria-label="推理模式选择">
        {MODE_OPTIONS.map((opt) => (
          <label
            key={opt.value}
            className={`onboarding__option ${selected === opt.value ? 'selected' : ''} ${opt.disabled ? 'disabled' : ''}`}
          >
            <input
              type="radio"
              name="inference-mode"
              value={opt.value}
              checked={selected === opt.value}
              disabled={opt.disabled}
              onChange={() => setSelected(opt.value)}
              className="onboarding__radio-input"
            />
            <span className="onboarding__radio-dot" aria-hidden />
            <div className="onboarding__option-content">
              <div className="onboarding__option-label">
                {opt.icon} {opt.label}
                {opt.tag && (
                  <span className="onboarding__tag" style={{ background: opt.tagColor }}>
                    {opt.tag}
                  </span>
                )}
              </div>
              <div className="onboarding__option-desc">{opt.desc}</div>
            </div>
          </label>
        ))}
      </div>

      <div className="onboarding__actions">
        <button
          type="button"
          className="btn btn--primary"
          data-testid="onboarding-next"
          onClick={() => {
            // Hybrid 是禁用占位，不会进入此分支；narrowing 为 InferenceMode
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
