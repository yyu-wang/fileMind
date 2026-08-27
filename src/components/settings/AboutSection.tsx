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
      <div className="settings-row">
        <div className="settings-field">
          <label className="settings-field__label">版本</label>
        </div>
        <span className="settings-embedding-meta" data-testid="about-version">
          {APP_VERSION}
        </span>
      </div>
      <div className="settings-row">
        <div className="settings-field">
          <label className="settings-field__label">技术栈</label>
        </div>
        <span className="settings-embedding-meta">
          Tauri 2 · React 19 · Python FastAPI · LangChain
        </span>
      </div>
      <div className="settings-row">
        <div className="settings-field">
          <label className="settings-field__label">索引数据库</label>
        </div>
        <span className="settings-embedding-meta">SQLite + LanceDB</span>
      </div>
      <div className="settings-row settings-row--actions">
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
    </section>
  );
}
