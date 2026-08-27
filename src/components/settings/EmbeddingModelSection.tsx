// Embedding 模型区块（设置页）：展示当前模型与可选模型可用性。
//
// 支持手动安装未安装的 Embedding 模型（从 Ollama 拉取）。
// 切换模型仍需重建索引，属后续建索引任务（T11+）。
// 原型 05_交互原型 §设置页 Embedding 管理：.panel 展示当前模型 + .model-item 列表。

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
        📐 Embedding 模型管理
      </h3>
      <p className="section-desc">切换 Embedding 模型需重建全部向量索引</p>

      <div className="panel" style={{ marginBottom: 12 }}>
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
          <div>
            <div style={{ fontSize: 14, fontWeight: 600, color: 'var(--ink)' }}>
              当前模型: {embeddingModel}
              {current ? ` · dim ${current.dim} · v${current.version}` : ''}
            </div>
            <div style={{ fontSize: 12, color: 'var(--muted)', marginTop: 2 }}>
              已索引 {indexedCount.toLocaleString()} 文件 · 向量块数未知
            </div>
          </div>
          <span className="tag tag-green" title="当前模型已锁定，切换需重建索引">
            已锁定
          </span>
        </div>
      </div>

      {installError && (
        <p className="section-desc" role="alert" style={{ color: 'var(--danger, #d33)' }}>
          安装失败: {installError}
        </p>
      )}

      <div className="section-desc" style={{ marginBottom: 8 }}>
        可切换模型：
      </div>
      {embeddingModelOptions.length === 0 ? (
        <div className="model-item">
          <div className="model-info">
            <div className="name">暂无模型列表</div>
            <div className="meta">请先检测 Ollama 环境</div>
          </div>
        </div>
      ) : (
        embeddingModelOptions.map((m) => {
          const isCurrent = m.name === embeddingModel;
          const isInstalling = installingModel === m.name;
          return (
            <div key={m.name} className={`model-item${isCurrent ? ' model-item--current' : ''}`}>
              <div className="model-info">
                <div className="name">{m.name}</div>
                <div className="meta">
                  dim {m.dim} · v{m.version}
                  {m.available ? '' : ' · 未安装'}
                </div>
              </div>
              {m.available ? (
                <span className="tag tag-green">已安装</span>
              ) : (
                <span className="tag tag-gray">未安装</span>
              )}
              {!m.available && (
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
              {isCurrent && m.available && (
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
            </div>
          );
        })
      )}
    </section>
  );
}
