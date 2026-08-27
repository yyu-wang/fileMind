// 关于区块（设置页）：版本 / 技术栈 / 索引数据库 + 检查更新 / 导出配置 / 重置应用。
//
// 原型 05_交互原型 §设置页关于区。纯静态展示 + 占位按钮。
// 版本号与 package.json 保持一致（手动同步），
// 「检查更新 / 导出配置 / 重置应用」P1 不支持，留 disabled 占位。

/** 应用版本：与 package.json 的 version 字段保持一致。 */
const APP_VERSION = 'v0.1.0';

export function AboutSection() {
  return (
    <section className="settings-section" aria-labelledby="settings-about-title">
      <h3 id="settings-about-title" className="settings-section__title">
        ℹ️ 关于
      </h3>
      <div className="setting-row">
        <div className="setting-label">
          <div className="name">版本</div>
        </div>
        <div className="setting-control">
          <span className="mono text-muted" data-testid="about-version">
            {APP_VERSION}
          </span>
        </div>
      </div>
      <div className="setting-row">
        <div className="setting-label">
          <div className="name">技术栈</div>
        </div>
        <div className="setting-control">
          <span className="text-muted text-sm">
            Tauri 2 · React 19 · Python FastAPI · LangChain
          </span>
        </div>
      </div>
      <div className="setting-row">
        <div className="setting-label">
          <div className="name">索引数据库</div>
        </div>
        <div className="setting-control">
          <span className="text-muted text-sm">SQLite + LanceDB</span>
        </div>
      </div>
      <div className="setting-row" style={{ borderBottom: 'none', padding: '12px 0 0' }}>
        <div className="setting-control" style={{ display: 'flex', gap: 10 }}>
          <button
            type="button"
            className="btn btn--ghost btn--sm"
            disabled
            title="P1 不支持，敬请期待"
          >
            检查更新
          </button>
          <button
            type="button"
            className="btn btn--ghost btn--sm"
            disabled
            title="P1 不支持，敬请期待"
          >
            导出配置
          </button>
          <button
            type="button"
            className="btn btn--ghost btn--sm"
            disabled
            title="P1 不支持，敬请期待"
            style={{ color: 'var(--warn)' }}
          >
            重置应用
          </button>
        </div>
      </div>
    </section>
  );
}
