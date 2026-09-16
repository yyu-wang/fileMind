// 「AI 模型配置」的占位态：尚无任何云提供商时展示。
//
// 从 CloudApiKeySection 抽出（原为组件顶部的 early return 分支）。

export function CloudApiKeyEmptyState() {
  return (
    <section className="settings-section" aria-labelledby="settings-api-key-title-empty">
      <h3 id="settings-api-key-title-empty" className="settings-section__title">
        🤖 AI 模型配置
      </h3>
      <p className="section-desc">
        请先在上方「云提供商管理」添加一个提供商，再回来配置 API Key 与模型。
      </p>
    </section>
  );
}
