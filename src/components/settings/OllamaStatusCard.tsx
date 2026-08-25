// Ollama 环境状态卡（设置页）：展示探测结果 + LLM 模型选择 + Embedding 可用性。
//
// 探测动作由父级（SettingsPage）挂载时触发，本卡只读 store 并支持手动重新探测；
// LLM 选择调用 setLlmModel 持久化；Embedding 仅展示可用性（真实切换属后续建索引任务）。

import { useState } from 'react';
import { useSettingsStore } from '../../stores/settingsStore';

export function OllamaStatusCard() {
  // FE-M5：模型切换失败的 inline 提示（store 已回滚旧值，此处只做可见性）
  const [modelSaveError, setModelSaveError] = useState<string | null>(null);
  const ollamaStatus = useSettingsStore((s) => s.ollamaStatus);
  const ollamaProbing = useSettingsStore((s) => s.ollamaProbing);
  const llmModel = useSettingsStore((s) => s.llmModel);
  const llmModelOptions = useSettingsStore((s) => s.llmModelOptions);
  const embeddingModelOptions = useSettingsStore((s) => s.embeddingModelOptions);
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
            Ollama 环境
          </h3>
          <p className="settings-section__desc">本地推理引擎状态与模型选择。</p>
        </div>
        <button
          type="button"
          className="btn btn--ghost"
          data-testid="ollama-redetect"
          onClick={() => void probeOllama()}
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

      <div className="settings-field">
        <label htmlFor="llm-model-select" className="settings-field__label">
          生成模型（LLM）
        </label>
        <select
          id="llm-model-select"
          className="settings-select"
          value={llmModel}
          onChange={(e) => {
            setModelSaveError(null);
            // FE-M5：此前 void 不 catch——持久化失败静默，下拉停在未保存的值上
            setLlmModel(e.target.value).catch(() => {
              setModelSaveError('模型保存失败，已还原为原模型，请重试');
            });
          }}
          disabled={!ollamaStatus?.available || ollamaProbing}
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
        <p className="settings-field__hint">聊天与智能分类使用的本地推理模型。</p>
        {modelSaveError && (
          <p className="settings-field__hint" role="alert" style={{ color: 'var(--danger, #d33)' }}>
            {modelSaveError}
          </p>
        )}
      </div>

      <div className="settings-field">
        <h4 className="settings-field__label">Embedding 模型可用性</h4>
        <ul className="settings-embedding-list">
          {embeddingModelOptions.length === 0 ? (
            <li className="settings-embedding-item">
              <span className="settings-embedding-name">暂无数据</span>
              <span className="settings-embedding-meta">未探测到模型列表</span>
            </li>
          ) : (
            embeddingModelOptions.map((m) => (
              <li key={m.name} className="settings-embedding-item">
                <span className="settings-embedding-name">{m.name}</span>
                <span className="settings-embedding-meta">
                  dim {m.dim} · v{m.version}
                </span>
                <span
                  className={`settings-embedding-badge ${m.available ? 'settings-embedding-badge--ok' : 'settings-embedding-badge--missing'}`}
                >
                  {m.available ? '已安装' : '未安装'}
                </span>
              </li>
            ))
          )}
        </ul>
      </div>
    </section>
  );
}
