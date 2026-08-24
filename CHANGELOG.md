# Changelog

本文件记录 FileMind 各版本的显著变更，格式遵循 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)。
任务编号（T\d+(\.\d+)?）对应 `项目开发文件/10_开发任务拆解与排期.html`。

草稿可从 `git log` 按任务 ID 分组自动生成：`python3 scripts/gen-changelog.py`。

## [Unreleased]

### Added

- **T0.8** 测试数据生成脚本：`scripts/gen_testdata.py`（可扩展 `--count`/`--depth`，供基准与 E2E 用）。
- **T1.4** Sidecar HMAC 握手协议与请求验签（防端口冒充）。
- **T1.5** Sidecar 生命周期管理：启动握手、健康看门狗、指数退避重启、CrashLoop 暂停告警。
- **T2.2–T2.4** 文档索引：LanceDB 向量索引 + 增量索引 + FTS5 中文分词，打通问答链路。
- **T2.5** 数据访问层 Repository pattern（OperationRepo / CategoryRepo / RuleRepo）。
- **T3.1** 扫描与索引：`scan_directory` IPC（路径校验 + 递归扫描 + SHA-256 hash + SQLite 落库）。
- **T3.2** 分类预览与执行：`preview_operations` 重写（batch_id + plan + summary + 4 冲突策略）、移动/复制、批量撤销。
- **T6.1** 窗口/托盘管理：关闭最小化到托盘 + 托盘「显示/退出」菜单，真退出优雅关停 Sidecar。
- **T6.4** 文件预览（图片/PDF 内容 base64 传输）。
- **T6.5** 分类规则引擎（扩展名/正则规则，规则编辑 UI）。
- **T6.6** SSE 流式代理：对话逐 token 流式输出。
- **T7.1–T7.5** 安全加固：日志脱敏、云端模式数据脱敏、云端 API Key 走 Keychain、Rust 云端代理、云端知情同意 UI。
- **T8.5** 本地/云端 Prompt 适配（Provider 工厂 + 云端变体 + Token 截断）。
- **T9.1/T9.2** CI/CD：PR 检查流水线（修复假通过）+ 合并构建流水线（真实交叉编译矩阵）。
- **T9.5** 前端 E2E 基建（WebdriverIO 嵌入式驱动驱动真实 debug 二进制）+ 规则/设置 E2E。
- **T10.2** RAG 首 token 优化：查询缓存 + Ollama keep_alive/num_ctx + httpx/LanceDB 连接复用 + TTFT 埋点。
- **T10.5** 基准脚本：`benchmarks/`（scan_perf + rag_bench + run-all 一键报告）。

### Fixed

- 分类移动/撤销后同步 LanceDB 索引路径，修复 RAG 引用失效。
- 建立文件索引误报序列化错误（校验非 2xx 响应并暴露真实错误）。
- 重复扫描保持文件 id 稳定；Sidecar 就绪超时放宽。
- 同模式切换幂等返回，避免引导流程卡住。
- Rerank 默认离线加载，规避打包后镜像下载 TLS 失败（HuggingFace 网络约束）。
- T9.1/T9.2 CI 构建前生成 specta IPC 类型（修复全新检出缺失导致的假通过）。

### Performance

- **T10.1** 增量扫描：mtime 快照跳过未变化文件 + 分块批量反查 + 锁外并行 SHA-256 hash。
- **T10.3** 内存门控 300→500MB + OMP 线程帽 + Rerank 空闲卸载。
- **T10.4** 虚拟滚动规模断言 + `selectableIds` 去 O(N)。

### Security

- 日志统一出口脱敏（Rust formatter + Python getLogger）。
- 云端模式 P-01/P-03 提示词出口接入脱敏门控。
- API Key 存 Keychain，不回传完整 Key。

### Tests

- 前端测试缺口补齐（ruleStore + 8 组件 + 5 页面），行覆盖率约 47.65% → 85.87%，共 289 个测试。
- Rust 集成测试：`tests/tray.rs`（托盘菜单纯决策 + quit→IS_QUITTING）、`tests/scan_perf.rs`（`--ignored` 基准门控）。
- 前端 E2E：`e2e/specs/004-rules.e2e.ts`、`005-settings.e2e.ts`（003 保持 `RUN_E2E=1 + --rag` 门控）。
