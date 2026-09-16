// 云端模型设置行：预设下拉输入 + 「保存模型设置」按钮。
//
// 从 CloudApiKeySection 抽出——模型名预设表与这两行 JSX 与 Key 行无关，
// 单独成组件后父级只透传值与回调。

//: 云端模型预设列表（仅作下拉示例，不绑定提供商）
const CLOUD_MODEL_OPTIONS = [
  // DeepSeek
  { value: 'deepseek-chat', label: 'DeepSeek Chat' },
  { value: 'deepseek-reasoner', label: 'DeepSeek Reasoner' },
  { value: 'deepseek-v3', label: 'DeepSeek V3' },
  // OpenAI
  { value: 'gpt-4o', label: 'GPT-4o' },
  { value: 'gpt-4o-mini', label: 'GPT-4o Mini' },
  { value: 'gpt-4.1', label: 'GPT-4.1' },
  { value: 'gpt-4.1-mini', label: 'GPT-4.1 Mini' },
  // Anthropic（通过兼容代理接入时使用）
  { value: 'claude-3-5-sonnet', label: 'Claude 3.5 Sonnet' },
  { value: 'claude-3-opus', label: 'Claude 3 Opus' },
  // Qwen
  { value: 'qwen-plus', label: 'Qwen Plus' },
  { value: 'qwen-turbo', label: 'Qwen Turbo' },
  // Moonshot
  { value: 'moonshot-v1-8k', label: 'Moonshot v1 8K' },
  // GLM
  { value: 'glm-4', label: 'GLM-4' },
];

interface CloudModelSettingRowProps {
  /** 输入框当前值 */
  value: string;
  /** 保存中（按钮禁用 + 文案切换） */
  saving: boolean;
  /** 输入框值变化 */
  onChange: (value: string) => void;
  /** 保存云端模型名 */
  onSave: () => void;
}

export function CloudModelSettingRow({
  value,
  saving,
  onChange,
  onSave,
}: CloudModelSettingRowProps) {
  return (
    <>
      <div className="setting-row">
        <div className="setting-label">
          <div className="name">云端模型</div>
          <div className="desc">API 推理使用的模型（选择预设或手动输入）</div>
        </div>
        <div className="setting-control">
          <input
            className="input"
            list="cloud-model-options"
            value={value}
            onChange={(e) => onChange(e.target.value)}
            placeholder="选择或输入模型名"
            aria-label="云端推理模型名"
            style={{ width: 260 }}
          />
          <datalist id="cloud-model-options">
            {CLOUD_MODEL_OPTIONS.map((opt) => (
              <option key={opt.value} value={opt.value}>
                {opt.label}
              </option>
            ))}
          </datalist>
        </div>
      </div>
      <div className="setting-row" style={{ borderBottom: 'none', paddingTop: 0 }}>
        <div className="setting-label" />
        <div className="setting-control">
          <button
            type="button"
            className="btn btn--primary btn--sm"
            disabled={saving}
            onClick={onSave}
          >
            {saving ? '保存中...' : '保存模型设置'}
          </button>
        </div>
      </div>
    </>
  );
}
