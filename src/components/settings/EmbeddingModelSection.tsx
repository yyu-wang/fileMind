// Embedding 模型区块（设置页）：展示当前模型与可选模型可用性。
//
// 支持手动安装未安装的 Embedding 模型（从 Ollama 拉取）。
// 切换模型仍需重建索引，属后续建索引任务（T11+）。

import { useEffect } from 'react';
import { useFileStore } from '../../stores/fileStore';
import { useSettingsStore } from '../../stores/settingsStore';

export function EmbeddingModelSection() {
  const embeddingModel = useSettingsStore((s) => s.embeddingModel);
  const embeddingModelOptions = useSettingsStore((s) => s.embeddingModelOptions);
  const installingModel = useSettingsStore((s) => s.installingModel);
  const installError = useSettingsStore((s) => s.installError);
  const installModel = useSettingsStore((s) => s.installModel);
  const ollamaAvailable = useSettingsStore((s) => s.ollamaStatus?.available ?? false);
  const stats = useFileStore((s) => s.stats);
  const loadStats = useFileStore((s) => s.loadStats);

  useEffect(() => {
    if (!stats) void loadStats();
  }, [stats, loadStats]);

  const current = embeddingModelOptions.find((m) => m.name === embeddingModel);
  const indexedCount = stats?.categorized_files ?? 0;

  return (
    <section className="settings-section" aria-labelledby="settings-embedding-title">
      <h3 id="settings-embedding-title" className="settings-section__title">
        Embedding 模型
      </h3>
      <p className="settings-section__desc">
        用于文件向量化的模型。切换模型需同步重建索引，将在后续版本提供。
      </p>

      <div className="settings-embedding-current-panel">
        <div className="settings-embedding-current-info">
          <div className="settings-embedding-current-name">
            当前模型: {embeddingModel}
            {current ? ` · dim ${current.dim} · v${current.version}` : ''}
          </div>
          <div className="settings-embedding-current-meta">
            已索引 {indexedCount.toLocaleString()} 文件 · 向量块数未知
          </div>
        </div>
        <span
          className="settings-embedding-badge settings-embedding-badge--ok"
          title="当前模型已锁定，切换需重建索引"
        >
          已锁定
        </span>
      </div>

      {installError && (
        <p className="settings-field__hint" role="alert" style={{ color: 'var(--danger, #d33)' }}>
          安装失败: {installError}
        </p>
      )}

      <div className="settings-section__desc">可切换模型：</div>
      <ul className="settings-embedding-list">
        {embeddingModelOptions.length === 0 ? (
          <li className="settings-embedding-item">
            <span className="settings-embedding-name">暂无模型列表</span>
            <span className="settings-embedding-meta">请先检测 Ollama 环境</span>
          </li>
        ) : (
          embeddingModelOptions.map((m) => {
            const isCurrent = m.name === embeddingModel;
            const isInstalling = installingModel === m.name;
            return (
              <li
                key={m.name}
                className={`settings-embedding-item ${isCurrent ? 'settings-embedding-item--current' : ''}`}
              >
                <div className="settings-embedding-info">
                  <span className="settings-embedding-name">{m.name}</span>
                  <span className="settings-embedding-meta">
                    dim {m.dim} · v{m.version}
                  </span>
                </div>
                <span
                  className={`settings-embedding-badge ${m.available ? 'settings-embedding-badge--ok' : 'settings-embedding-badge--missing'}`}
                >
                  {m.available ? '已安装' : '未安装'}
                </span>
                {!m.available && !isCurrent && (
                  <button
                    type="button"
                    className="btn btn--primary btn--sm"
                    disabled={isInstalling || !ollamaAvailable}
                    onClick={() => void installModel(m.name)}
                    title={ollamaAvailable ? '点击安装此模型' : 'Ollama 不可用，请先启动 Ollama'}
                  >
                    {isInstalling ? '安装中...' : '安装'}
                  </button>
                )}
                {isCurrent && (
                  <button
                    type="button"
                    className="btn btn--ghost btn--sm"
                    disabled
                    title="当前模型，切换需重建全部向量索引，敬请期待"
                  >
                    当前
                  </button>
                )}
                {m.available && !isCurrent && (
                  <button
                    type="button"
                    className="btn btn--ghost btn--sm"
                    disabled
                    title="切换需重建全部向量索引，敬请期待"
                  >
                    切换
                  </button>
                )}
              </li>
            );
          })
        )}
      </ul>
    </section>
  );
}
