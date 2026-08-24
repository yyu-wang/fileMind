// Embedding 模型区块（设置页）：展示当前模型与可选模型可用性。
//
// 仅展示 + 预检查，不落库切换：向量索引表名按
// `documents_{embedding_model}_v{version}` 命名，切换需同步重建索引，
// 属后续建索引任务。RAG 表名一致性由该约束保证。

import { useSettingsStore } from '../../stores/settingsStore';

export function EmbeddingModelSection() {
  const embeddingModel = useSettingsStore((s) => s.embeddingModel);
  const embeddingModelOptions = useSettingsStore((s) => s.embeddingModelOptions);
  const ollamaProbing = useSettingsStore((s) => s.ollamaProbing);

  const current = embeddingModelOptions.find((m) => m.name === embeddingModel);

  return (
    <section className="settings-section" aria-labelledby="settings-embedding-title">
      <h3 id="settings-embedding-title" className="settings-section__title">
        Embedding 模型
      </h3>
      <p className="settings-section__desc">
        用于文件向量化的模型。切换模型需同步重建索引，将在后续版本提供。
      </p>

      <div className="settings-row">
        <span
          className="settings-mode-badge"
          title="当前 Embedding 模型"
          data-testid="embedding-current"
        >
          {embeddingModel}
          {current ? ` · dim ${current.dim} · v${current.version}` : ''}
        </span>
        <span
          className={`settings-embedding-badge ${current?.available ? 'settings-embedding-badge--ok' : 'settings-embedding-badge--missing'}`}
        >
          {ollamaProbing ? '检测中' : current ? (current.available ? '已安装' : '未安装') : '未知'}
        </span>
      </div>

      <ul className="settings-embedding-list">
        {embeddingModelOptions.length === 0 ? (
          <li className="settings-embedding-item">
            <span className="settings-embedding-name">暂无模型列表</span>
            <span className="settings-embedding-meta">请先检测 Ollama 环境</span>
          </li>
        ) : (
          embeddingModelOptions.map((m) => (
            <li
              key={m.name}
              className={`settings-embedding-item ${m.name === embeddingModel ? 'settings-embedding-item--current' : ''}`}
            >
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
    </section>
  );
}
