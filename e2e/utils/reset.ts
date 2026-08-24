// T9.5 E2E：测试助手——前端状态清理、等待、fixture 路径读取。
//
// 说明：测试隔离（SQLite + LanceDB）由 e2e-run.sh 通过每个 spec 独立进程 +
// 全新 FILEMIND_DATA_HOME 保证；resetFrontendState 只清 zustand persist 写到的
// localStorage / sessionStorage（onboarding 完成标记），避免跨 spec 串态。

/** 清空 WebView 前端持久化并复位滚动。 */
export async function resetFrontendState(): Promise<void> {
  await browser.execute(() => {
    localStorage.clear();
    sessionStorage.clear();
    window.scrollTo(0, 0);
  });
}

/** E2E fixture 扫描目录（e2e-run.sh 注入 FILEMIND_E2E_DATA_DIR；spec 进程可直接读）。 */
export function e2eDataDir(): string {
  const dir = process.env.FILEMIND_E2E_DATA_DIR;
  if (!dir) throw new Error('缺少 FILEMIND_E2E_DATA_DIR（e2e-run.sh 应注入）');
  return dir;
}

/**
 * 把知情同意书滚动到底，触发 onBottomReached 解锁 checkbox。
 *
 * 设置 scrollTop 后 WebView 会异步派发原生 scroll 事件；这里同时手动派发
 * 合成事件兜底（对齐 ConsentAgreement.handleScroll 的判断：距底 ≤ tolerance）。
 */
export async function scrollConsentToBottom(): Promise<void> {
  await browser.execute(() => {
    const el = document.querySelector<HTMLElement>('.consent-agreement__scroll');
    if (el) {
      el.scrollTop = el.scrollHeight;
      el.dispatchEvent(new Event('scroll', { bubbles: true }));
    }
  });
}
