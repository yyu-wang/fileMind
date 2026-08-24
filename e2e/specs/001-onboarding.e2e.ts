// T9.5 E2E-001 首次引导（真实后端，全新数据目录，e2e-run.sh 注入隔离 env）。
//
// 流程：引导 dialog → 默认「本地模式」选中 → 切「云端模式」→ 同意书滚动到底 →
// 勾选确认 → 目录步自动填充（getE2eTestDir 钩子）→ 开始使用 → 主界面
// （sidebar + 文件页标题）。
//
// 关键点：
//   - 应用文案是「本地模式/云端模式」（设计稿规格写的是「本地推理」，以实际 DOM 为准）。
//   - 同意书 checkbox 需滚动到底才解锁（T7.5 滚动门控）。
//   - 目录步依赖原生 dialog 不可点，由 main.rs 预置的 `e2e_get_test_dir` 自动填充。

import { expect } from '@wdio/globals';

import { sel } from '../utils/selectors';
import { scrollConsentToBottom } from '../utils/reset';

describe('E2E-001 首次引导', () => {
  it('走完首次引导并进入主界面', async () => {
    // —— 1. 引导 dialog 出现（sidecar 冷启动后应用就绪）——
    const wizard = $(sel.onboardingWizard);
    await wizard.waitForExist({ timeout: 120000 });
    await expect(wizard).toBeExisting();

    // —— 2. 默认「本地模式」选中 ——
    await expect($(sel.onboardingModeLocal)).toBeChecked();
    await expect($(sel.onboardingModeCloud)).not.toBeChecked();

    // —— 3. 切「云端模式」→ 进入同意书 ——
    await $('.onboarding__option*=云端模式').click();
    const nextBtn = $(sel.onboardingNext);
    await nextBtn.waitForClickable();
    await nextBtn.click();

    // —— 4. 同意书：未滚到底 checkbox 禁用 → 滚动到底解锁 → 勾选确认 ——
    const consentCheck = $(sel.onboardingConsentCheck);
    await consentCheck.waitForExist();
    await expect(consentCheck).toBeDisabled();
    await scrollConsentToBottom();
    // 滚动后 React 状态更新需要一帧
    await expect(consentCheck).toBeEnabled({ wait: 2000 });
    await consentCheck.click();
    await expect(consentCheck).toBeChecked();
    await $(sel.onboardingConsentConfirm).click();

    // —— 5. 目录步：测试目录自动填充（getE2eTestDir 钩子跳过原生 dialog）——
    const selectedDir = $(sel.onboardingSelectedDir);
    await selectedDir.waitForExist({ timeout: 10000 });
    // fixture 目录是 e2e-run.sh 的 mktemp（前缀 fm-e2e-scan）；正则局部匹配
    await expect(selectedDir).toHaveText(/fm-e2e-scan/);

    // —— 6. 开始使用 → 主界面（sidebar + 文件页标题），首次扫描自动触发 ——
    const startBtn = $(sel.onboardingStart);
    await expect(startBtn).toBeEnabled();
    await startBtn.click();
    await $(sel.sidebar).waitForExist({ timeout: 30000 });
    await expect($(sel.sidebar)).toBeExisting();
    await expect($(sel.filesTitle)).toHaveText('文件管理');
  });
});
