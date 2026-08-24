// T9.5 spike：验证 WDIO + 嵌入式驱动链路。
// 断言真实 debug 应用启动后能渲染首次启动引导 dialog。

import { expect } from '@wdio/globals';

describe('spike: embedded WebDriver 链路', () => {
  it('应用启动并显示首次启动引导', async () => {
    const wizard = $('[role="dialog"][aria-label="首次启动引导"]');
    await wizard.waitForExist({ timeout: 90000 });
    await expect(wizard).toBeExisting();
  });

  it('browser.execute 可用（E2E-001 需要清 localStorage / 滚动同意书）', async () => {
    // 嵌入式驱动 execute 是 W3C 合规的；断言能读写 DOM
    const readyState = await browser.execute(() => document.readyState);
    await expect(readyState).toBe('complete');

    const setGet = await browser.execute(() => {
      localStorage.setItem('spike-key', 'ok');
      return localStorage.getItem('spike-key');
    });
    await expect(setGet).toBe('ok');
  });
});
