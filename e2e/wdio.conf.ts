// T9.5 E2E 配置：WebdriverIO + @wdio/tauri-service（嵌入式 WebDriver，驱动真实 debug 二进制）。
//
// 测试隔离由 scripts/e2e-run.sh 保证：每个 spec 单独一个 wdio 进程 + 各自全新的
// FILEMIND_DATA_HOME（SQLite+LanceDB 完整隔离）与 FILEMIND_E2E_DATA_DIR（fixture 目录）。
// 应用由 embedded provider 直接 spawn，继承本进程 env。
//
// 已知良性噪音（不影响通过，勿当失败排查）：
//   - `ERROR diagnostics: tauri-driver not found`：服务在诊断外部 tauri-driver，但我们用
//     embedded provider（应用内 WKWebView/WebKitGTK 起 W3C server），无需外部驱动。
//   - `WARN Failed to get window states: wdio.get_window_states not allowed`：macOS 上
//     tauri-plugin-wdio-webdriver 未实现 get_window_states 命令，invoke 毫秒级失败 →
//     跳过 focus-check。此前该调用因 `__wdio_original_core__` 未注入会干等 5s 拖垮全部测试，
//     已在 src/main.tsx 加 shim 修复（毫秒级失败）。

import { resolve } from 'node:path';

export const config = {
  runner: 'local',
  specs: ['./e2e/specs/**/*.e2e.ts'],
  // 刻意不设 exclude：spec 选择一律交给 scripts/e2e-run.sh 的 case 门控 + `--spec`。
  // 两个已踩过的坑，勿回退成 exclude：
  //   1. WDIO 的 `--spec` 同样会被 exclude 过滤（filterSpecs 做精确路径比对），
  //      加 exclude 等于让被排除的 spec 永远跑不起来（--rag / RUN_E2E 会静默失效）。
  //   2. 本 config 的 rootDir 是配置文件所在目录 `e2e/`，所以上面这条 specs（以及
  //      任何 `./e2e/...` 写法的 exclude）其实都匹配不到文件——启动日志里那句
  //      `pattern ./e2e/specs/**/*.e2e.ts did not match any file` 即由此而来。
  //      真正生效的 spec 列表全部来自 e2e-run.sh 传入的绝对路径 `--spec`。
  maxInstances: 1,
  maxInstancesPerCapability: 1,

  capabilities: [
    {
      browserName: 'tauri',
      'tauri:options': {
        application: resolve('src-tauri/target/debug/filemind'),
      },
    },
  ],

  services: [
    [
      '@wdio/tauri-service',
      {
        // embedded：应用内 tauri-plugin-wdio-webdriver 起 WebDriver server，无需外部 tauri-driver
        driverProvider: 'embedded',
        embeddedPort: 4445,
        captureBackendLogs: true,
        captureFrontendLogs: true,
      },
    ],
  ],

  logLevel: 'info',
  bail: 0,
  waitforTimeout: 20000,
  connectionRetryTimeout: 120000, // 应用冷启动 + sidecar 握手最坏 ~40s，必须放宽
  connectionRetryCount: 3,

  framework: 'mocha',
  mochaOpts: {
    ui: 'bdd',
    // 默认 180s；E2E-006 要等一次真实 313MB 模型下载，由 scripts/e2e-run.sh 用
    // FILEMIND_E2E_MOCHA_TIMEOUT 单独上调（写在 config 里会波及全部 spec）
    timeout: Number(process.env.FILEMIND_E2E_MOCHA_TIMEOUT ?? 180000),
    // 冒烟保险：应用冷启动（sidecar ~30-40s + 嵌入式驱动）是唯一偶发点，
    // 单个用例失败重跑 1 次，避免 CI 因一次冷启动抖动误报
    retries: 1,
  },

  reporters: ['spec'],
};
