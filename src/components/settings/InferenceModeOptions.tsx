// 推理模式选择区（设置页 · 对齐交互原型 §设置 §推理模式）：
// mode-selector(local/cloud) + radio-dot 选中态。
// 混合模式（hybrid）P1 未开发，不渲染占位选项。
//
// 文档结构：
//   <div class="mode-selector" role="radiogroup">
//     <div class="mode-option local selected" role="radio" aria-checked="true">
//       <div class="mode-icon">🛡️</div>
//       <div class="mode-info"><div class="name">本地模式（当前）</div>...</div>
//       <div class="radio-dot"></div>
//     </div>
//     <div class="mode-option cloud">...</div>
//   </div>
//
// 与 InferenceModeSection 分离的原因：这是独立视觉区域（选项数据、每行的 role=radio
// 与键盘可达性、「已签署同意」tag 同属一块），与区块底部的同意书流程无关。

import type { ModeValue } from './useInferenceModeSwitch';

interface ModeOption {
  value: ModeValue;
  icon: string;
  name: string;
  desc: string;
}

const MODE_OPTIONS: ModeOption[] = [
  {
    value: 'local',
    icon: '🛡️',
    name: '本地模式',
    desc: 'Ollama 本地推理 · 数据不离开设备',
  },
  {
    value: 'cloud',
    icon: '☁️',
    name: '云端模式',
    desc: 'API 推理 · 文件内容上传第三方',
  },
];

interface ModeOptionRowProps {
  /** 该行的静态选项数据 */
  option: ModeOption;
  /** 是否为当前选中项（决定 selected / aria-checked） */
  selected: boolean;
  /** 云端模式且已签署同意（仅云端行展示 tag） */
  cloudSigned: boolean;
  /** 点击或回车该选项 */
  onSelect: (value: ModeValue) => void;
}

/** 单个 mode-option 行（radiogroup 中的 role=radio 项）。 */
function ModeOptionRow({ option, selected, cloudSigned, onSelect }: ModeOptionRowProps) {
  const showSignedTag = option.value === 'cloud' && cloudSigned;
  return (
    <div
      className={`mode-option ${option.value}${selected ? ' selected' : ''}`}
      role="radio"
      aria-checked={selected}
      aria-label={`${option.icon} ${option.name}${selected ? '（当前）' : ''}`}
      data-testid={`mode-option-${option.value}`}
      tabIndex={0}
      onClick={() => onSelect(option.value)}
      onKeyDown={(e) => {
        if (e.key === 'Enter' || e.key === ' ') {
          e.preventDefault();
          onSelect(option.value);
        }
      }}
    >
      <div className="mode-icon" aria-hidden>
        {option.icon}
      </div>
      <div className="mode-info">
        <div className="name">
          {option.name}
          {selected && '（当前）'}
        </div>
        <div className="desc">
          {option.desc}
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
}

export interface InferenceModeOptionsProps {
  /** 当前生效的模式 */
  current: ModeValue;
  /** 云端模式且已签署同意（云端行的 desc 后追加「已签署同意」tag） */
  cloudSigned: boolean;
  /** 点击某个 mode-option（切换语义由调用方处理） */
  onSelect: (value: ModeValue) => void;
}

export function InferenceModeOptions({
  current,
  cloudSigned,
  onSelect,
}: InferenceModeOptionsProps) {
  return (
    <div className="mode-selector" role="radiogroup" aria-label="推理模式">
      {MODE_OPTIONS.map((opt) => (
        <ModeOptionRow
          key={opt.value}
          option={opt}
          selected={opt.value === current}
          cloudSigned={cloudSigned}
          onSelect={onSelect}
        />
      ))}
    </div>
  );
}
