// 设置页的「字段行」：左侧标签与说明，右侧输入框与字段级错误提示。
//
// 从 CloudProviderFormCard 抽出——这段结构（含错误类名与提示显隐）原先手写了五遍。
// 组件不感知表单状态：「是否已触摸该字段」的判断由父级完成，本组件只在 error 非空时
// 展示提示并把输入框标红。

import type { ChangeEvent } from 'react';

interface SettingsInputRowProps {
  /** 字段标签 */
  label: string;
  /** 标签下的灰色说明（不传则不渲染） */
  desc?: string;
  /** 当前值 */
  value: string;
  /** 值变化（父级可在此做规范化，如转小写、去尾空格） */
  onChange: (value: string) => void;
  /** 失焦（父级据此标记该字段已触摸） */
  onBlur?: () => void;
  /** 错误文案；非空即展示提示并标红输入框 */
  error?: string | null;
  /** 占位文案 */
  placeholder?: string;
  /** 无障碍名称（输入框的 aria-label） */
  ariaLabel?: string;
  /** 是否禁用 */
  disabled?: boolean;
  /** 输入框宽度（px） */
  width?: number;
  /** 是否去掉行底边框（末行使用） */
  last?: boolean;
}

export function SettingsInputRow({
  label,
  desc,
  value,
  onChange,
  onBlur,
  error = null,
  placeholder,
  ariaLabel,
  disabled = false,
  width = 260,
  last = false,
}: SettingsInputRowProps) {
  const handleChange = (evt: ChangeEvent<HTMLInputElement>) => {
    onChange(evt.target.value);
  };

  return (
    <div className="setting-row" style={last ? { borderBottom: 'none' } : undefined}>
      <div className="setting-label">
        <div className="name">{label}</div>
        {desc && <div className="desc">{desc}</div>}
      </div>
      <div className="setting-control">
        <input
          className={`input${error ? ' input--error' : ''}`}
          value={value}
          disabled={disabled}
          onChange={handleChange}
          onBlur={onBlur}
          placeholder={placeholder}
          aria-label={ariaLabel}
          style={{ width }}
        />
        {error && (
          <p className="settings-section__error" role="alert" style={{ marginTop: 4 }}>
            {error}
          </p>
        )}
      </div>
    </div>
  );
}
