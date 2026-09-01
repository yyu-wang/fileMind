// T9.5 E2E-005 设置：Ollama 探测、主题切换、Embedding 当前模型徽标（真实后端）。
//
// 流程：直达文件页（SKIP_ONBOARDING）→ 侧栏进设置 → Ollama 状态区呈现探测结果 →
// 暗色主题切换（aria-pressed + html[data-theme]）→ Embedding 当前模型徽标。
//
// 说明：Ollama 可用性依赖本机真实环境（sidecar 探测），只断言「探测结束呈现
// 可用/不可用」而不限定具体值，保证 CI 与本地都稳定。

import { expect } from '@wdio/globals';

import { sel } from '../utils/selectors';

const SIDEBAR_SETTINGS = `${sel.sidebarItem}[title*="设置"]`;

describe('E2E-005 设置', () => {
  it('进入设置并完成 Ollama 探测', async () => {
    await $(sel.filesTitle).waitForExist({ timeout: 120000 });
    await $(SIDEBAR_SETTINGS).click();
    await $(sel.settingsTitle).waitForExist({ timeout: 15000 });

    // 挂载触发 probeOllama → 结束态为 可用 或 不可用（本机 Ollama 决定）
    await browser.waitUntil(
      async () => {
        const text = await $(sel.ollamaStatus).getText();
        return text.includes('Ollama 可用') || text.includes('Ollama 不可用');
      },
      { timeout: 30000, timeoutMsg: 'Ollama 探测应结束并呈现可用/不可用' },
    );
    // toExist 而非 toBeDisplayed：WKWebView 605.x 的 checkVisibility 在 .btn--ghost
    // 上偶发返回 false（display:block 但判定不可见），导致 expect().toBeDisplayed()
    // 在 waitforTimeout 内反复 stale 重试 → 首轮失败；retry 时已在设置页，filesTitle
    // 永不存在 → 120s 超时。toExist 只验 DOM 存在，规避 checkVisibility 误判。
    await expect($(sel.ollamaRedetect)).toExist();
  });

  it('重新检测按钮触发新一轮探测', async () => {
    await $(sel.ollamaRedetect).waitForExist({ timeout: 15000 });
    await $(sel.ollamaRedetect).click();
    // 探测瞬时结束，最终仍收敛到 可用/不可用
    await browser.waitUntil(
      async () => {
        const text = await $(sel.ollamaStatus).getText();
        return text.includes('Ollama 可用') || text.includes('Ollama 不可用');
      },
      { timeout: 30000, timeoutMsg: '重新检测后应回到 可用/不可用' },
    );
  });

  it('暗色主题切换应用到 html data-theme', async () => {
    await $(sel.themeDark).waitForExist({ timeout: 10000 });
    await $(sel.themeDark).click();
    await expect($(sel.themeDark)).toHaveAttr('aria-pressed', 'true');
    const theme = await browser.execute(() => document.documentElement.getAttribute('data-theme'));
    expect(theme).toBe('dark');
  });

  it('展示当前 Embedding 模型徽标', async () => {
    await $(sel.embeddingCurrent).waitForExist({ timeout: 10000 });
    // 徽标展示模型名（bge-small-zh 默认），可用性未知/已安装由本机决定
    await expect($(sel.embeddingCurrent)).toHaveText(/bge-/);
  });
});
