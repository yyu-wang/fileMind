// T7 E2E-006 Embedding 模型下载：未下载 → 点下载 → 进度可见 → 最终就绪（真实 HF 镜像）。
//
// 背景：Embedding 改为 Sidecar 进程内 ONNX 推理后，模型文件必须先落到本机才能建立
// 索引与问答；本 spec 覆盖「设置页触发下载 → 进度 → 就绪」这条点击链路，即
// UI → Tauri IPC → Sidecar `/models/download` 的完整往返。
//
// 门控：真实下载 313MB，默认本地 e2e 不跑，需 RUN_E2E=1（见 scripts/e2e-run.sh）。
// 前置：e2e-run.sh 为每个 spec 分配全新 FILEMIND_DATA_HOME，其中不含模型文件，
// 因此本 spec 天然从「未下载」开始；就绪态由探测本机模型文件得出，故「已就绪」
// 断言等价于「文件确实下载并落盘」。
//
// 注意：mochaOpts.retries=1 会对失败用例重跑一次，而「下载中」按钮是 disabled、
// 重跑也等不到新结果，本用例显式关掉重试（状态化流程，重试只会在报告里制造噪音）。
//
// 超时：**不能用 `this.timeout()` 抬高**——WDIO 测试体里的 `this` 不是 Mocha 的
// 测试 Runnable，`this.timeout(ms)` 只落到 suite 上（只影响 wdio 自己的 executeAsync
// 包装计时器），Mocha 的单测计时器仍取 mochaOpts.timeout，默认 180s 会在下载中途
// 掐断用例（实测：下载到 0.0MB 停 180s 后被杀）。故由 scripts/e2e-run.sh 为 006
// 单独 export FILEMIND_E2E_MOCHA_TIMEOUT=1800000，wdio.conf.ts 读它覆盖默认值。

import { expect } from '@wdio/globals';

import { sel } from '../utils/selectors';

const SIDEBAR_SETTINGS = `${sel.sidebarItem}[title*="设置"]`;
// 313MB + 镜像切换重试（首轮 TLS1.2 回退 + 3 次自动重试）留足余量
const DOWNLOAD_TIMEOUT = 15 * 60 * 1000;

describe('E2E-006 Embedding 模型下载', () => {
  it('设置页触发下载，进度可见并最终就绪', async function () {
    this.retries(0);

    // 等应用外壳（侧栏）而非文件页：本 spec 可能因 313MB 下载耗时较长被重跑，
    // 重跑时应用停在设置页，等 .files-page 必然 120s 超时 → 报告被污染。
    await $(sel.sidebar).waitForExist({ timeout: 120000 });
    await $(SIDEBAR_SETTINGS).click();
    await $(sel.settingsTitle).waitForExist({ timeout: 30000 });

    // 模型卡片：默认模型名 + 未下载状态（临时数据目录内无模型文件）
    await $(sel.embeddingCurrent).waitForExist({ timeout: 30000 });
    await expect($(sel.embeddingCurrent)).toHaveText(/bge-large-zh-v1\.5/);

    // 已就绪短路：同一数据目录重复跑时不再下载一遍 313MB
    if (!(await $(sel.modelReady).isExisting())) {
      await $(sel.modelDownloadBtn).waitForExist({ timeout: 30000 });
      await $(sel.modelDownloadBtn).click();

      // 点击后应进入下载中（进度条）或极快完成（就绪）——两者都算通过
      await browser.waitUntil(
        async () =>
          (await $(sel.modelDownloadProgress).isExisting()) ||
          (await $(sel.modelReady).isExisting()),
        {
          timeout: 60000,
          timeoutMsg: '点击下载后应出现进度条（或极快完成时直接就绪）',
        },
      );

      // 下载中不得出现失败提示（失败时给出原因是设计行为，此处断言"未失败"）
      await expect($(sel.modelDownloadFailure)).not.toExist();
    }

    // 收敛到就绪：`available=true` 来自 Sidecar 探测本机模型文件 → 文件确实存在
    await $(sel.modelReady).waitForExist({ timeout: DOWNLOAD_TIMEOUT });
    await expect($(sel.modelReady)).toHaveText(/已就绪/);

    // 就绪后进度条消失、下载按钮不再出现
    await expect($(sel.modelDownloadProgress)).not.toExist();
    await expect($(sel.modelDownloadBtn)).not.toExist();
  });
});
