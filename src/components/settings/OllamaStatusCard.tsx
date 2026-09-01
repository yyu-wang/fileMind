// Ollama 环境状态卡（设置页）：展示探测结果 + LLM 模型选择。
//
// 探测动作由父级（SettingsPage）挂载时触发，本卡只读 store 并支持手动重新探测；
// LLM 选择调用 setLlmModel 持久化。
// 原型 05_交互原型 §设置页 AI 模型配置：.setting-row + .setting-label + .setting-control。

import { useState } from 'react';
import { useSettingsStore } from '../../stores/settingsStore';

export function OllamaStatusCard() {
  // FE-M5：模型切换失败的 inline 提示（store 已回滚旧值，此处只做可见性）
  const [modelSaveError, setModelSaveError] = useState<string | null>(null);
  const ollamaStatus = useSettingsStore((s) => s.ollamaStatus);
  const ollamaProbing = useSettingsStore((s) => s.ollamaProbing);
  const llmModel = useSettingsStore((s) => s.llmModel);
  const llmModelOptions = useSettingsStore((s) => s.llmModelOptions);
  const probeOllama = useSettingsStore((s) => s.probeOllama);
  const setLlmModel = useSettingsStore((s) => s.setLlmModel);

  const badge = ollamaProbing
    ? { cls: 'settings-status--probing', text: '⏳ 正在检测 Ollama...' }
    : ollamaStatus?.available
      ? { cls: 'settings-status--ok', text: '● Ollama 可用' }
      : { cls: 'settings-status--down', text: '○ Ollama 不可用' };

  return (
    <section className="settings-section" aria-labelledby="settings-ollama-title">
      <div className="settings-section__head">
        <div>
          <h3 id="settings-ollama-title" className="settings-section__title">
            🔧 Ollama 环境
          </h3>
          <p className="section-desc">本地推理引擎状态与模型选择。</p>
        </div>
        <button
          type="button"
          className="btn btn--ghost btn--sm"
          data-testid="ollama-redetect"
          onClick={() => void probeOllama(true)}
          disabled={ollamaProbing}
        >
          重新检测
        </button>
      </div>

      <div className={`settings-status ${badge.cls}`} role="status">
        {badge.text}
        {!ollamaProbing && !ollamaStatus?.available && ollamaStatus?.message && (
          <span className="settings-status__detail">{ollamaStatus.message}</span>
        )}
      </div>

      <div className="setting-row">
        <div className="setting-label">
          <div className="name">本地模型</div>
          <div className="desc">Ollama 运行的 LLM 模型</div>
        </div>
        <div className="setting-control">
          <select
            id="llm-model-select"
            className="input"
            value={llmModel}
            onChange={(e) => {
              setModelSaveError(null);
              // FE-M5：此前 void 不 catch——持久化失败静默，下拉停在未保存的值上
              setLlmModel(e.target.value).catch(() => {
                setModelSaveError('模型保存失败，已还原为原模型，请重试');
              });
            }}
            disabled={!ollamaStatus?.available || ollamaProbing}
            aria-label="本地 LLM 模型"
            style={{ width: 220 }}
          >
            {ollamaStatus?.available ? (
              <>
                {/* 当前模型不在已安装列表时（配置残留），仍作为可选项展示 */}
                {llmModelOptions.some((m) => m.name === llmModel) ? null : (
                  <option value={llmModel}>{llmModel}</option>
                )}
                {llmModelOptions.map((m) => (
                  <option key={m.name} value={m.name}>
                    {m.name}
                  </option>
                ))}
              </>
            ) : (
              <option value={llmModel}>{llmModel}</option>
            )}
          </select>
        </div>
      </div>

      {modelSaveError && (
        <p className="section-desc" role="alert" style={{ color: 'var(--danger, #d33)' }}>
          {modelSaveError}
        </p>
      )}
    </section>
  );
}
