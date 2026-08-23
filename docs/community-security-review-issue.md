# Community Security Review Request

> 本文档是 T7.6 社区安全 review Issue 的草稿。发布前请：
>
> 1. ✅ 仓库地址 / 分支 / commit 已填（`dev` @ `701b183`）
> 2. 勾选确认发布前已重跑自动化安全测试
> 3. 逐条执行报告「未自动化项汇总」中的人工验证

---

## 标题

**Security review requested: FileMind v1.0 — 请求社区安全审计（STRIDE 自审已全通过）**

## 正文

### 背景

FileMind 是一个本地优先的桌面文件智能管理应用（Tauri 2 + Rust 后端，Python 3.12 FastAPI Sidecar，React 前端）。它索引本地文件、做分类与 RAG 问答。出于隐私考虑，我们内置了多层安全机制，并完成了 STRIDE 驱动的安全自审。

由于本项目是**个人开发者的开源项目，没有专业安全审计团队**，我们请求社区进行安全 review 作为补充防线。

**审查基线**：分支 `dev`，commit `701b183`（T7.5 知情同意完成，T7.1~T7.5 安全任务全部落库）。

### 技术栈

| 层       | 技术                                                                    |
| -------- | ----------------------------------------------------------------------- |
| 桌面壳   | Tauri 2 (Rust) — 前端侧进程，负责 Keychain 存取、云端代理转发           |
| 本地推理 | Python 3.12 FastAPI Sidecar + Ollama + LanceDB（本地向量库）            |
| 云端推理 | Rust 内嵌 axum 代理 → OpenAI / DeepSeek（共享 token 鉴权 + URL 白名单） |
| 前端     | React 19 + TypeScript strict + Zustand                                  |

### 自审结论

安全自审报告（STRIDE 六维，SEC-001~013，13/13 通过）见：
`docs/security-self-audit.md`

**主要缓解措施（对应各任务）**：

1. **T7.1 日志脱敏** — 统一日志出口正则脱敏：API Key（OpenAI 风格 / Bearer / key=value）、完整文件路径；脱敏初始化失败即退出，不进入无脱敏运行态
2. **T7.2 云端数据脱敏** — 云端模式仅发送内容前 500 字符摘要、文件名编号化（file_001）、相对路径；由 Rust 注入 `FILEMIND_CLOUD_MASKING=1` 门控
3. **T7.3 Keychain 存取** — API Key 只存 OS Keychain，前端仅获末 4 位掩码 hint，完整 Key 永不回传
4. **T7.4 Rust 云端代理** — Sidecar → Rust → 云端单向代理，Key 不进 Python 进程；URL 白名单（仅 OpenAI/DeepSeek 官方域名）；Key 用后 `zeroize`；`unsafe_code=deny`
5. **T7.5 知情同意书** — 切云端必须「滚动到底 + 勾选」双条件确认，同意书逐项列明实际发送范围；可一键撤回
6. **既有安全基线** — 启动握手（PSK 经 stdin 注入 + HMAC-SHA256 验签）、HMAC 请求签名防中间人/重放、操作日志链式哈希 + 触发器防篡改、路径遍历/符号链接防护、watchdog 自动重启

### 自动化测试现状（已通过）

- Rust `cargo test`：**253 passed**
- Python 安全相关：**61 passed**（握手 / HMAC / 云端脱敏 / 索引删除 / 超大文件 / 日志脱敏）
- 前端同意书门控：**14 passed**

### 请求的 review 范围

重点：**Rust 侧**（Tauri 后端 + axum 代理）、**Sidecar 鉴权链路**、**Key 生命周期**。

- [ ] Rust 云端代理（`src-tauri/src/security/cloud_proxy.rs`）：鉴权、URL 白名单、Key 零化时机、错误信息是否泄露敏感信息
- [ ] Keychain 存取链路（`api_key.rs`）：mask hint 计算、错误处理、是否有 Key 落入日志/DB 的路径
- [ ] 启动握手（`handshake.rs`）与 HMAC 中间件（`hmac_middleware.py`）：PSK 分发、防重放、时间窗口
- [ ] 日志脱敏（`log_redact.rs` / `logging_mask.py`）：绕过正则的形式是否可导致 Key/路径泄露
- [ ] 路径防护（`path_guard.rs`）：遍历 / 符号链接 / 大小写差异 / 规范化绕过
- [ ] 操作日志链（`log_chain.rs` + SQLite 触发器）：可绕过性
- [ ] 数据最小化（`cloud_mask.py`）：是否有未脱敏字段随云端请求发出
- [ ] 知情同意（前端 `ConsentAgreement`）：门控是否可被绕过（如缩放/注入事件）

### 约束与说明

- **不接受 DDoS / 资源耗尽类**的攻击测试（本机应用，非公网服务）
- 若发现漏洞，请先**私有披露**（见下），修复前请勿公开 PoC
- 本 Issue 发布时已确认：自动化安全测试全通过；8 项人工验证（tcpdump 抓包、kill 重启、符号链接等）**待 v1.0 发布前执行**，可随时要求补充结果

### 如何开始

```bash
git clone https://github.com/yyu-wang/fileMind.git
git checkout dev
# 构建与运行指引见 README / docs
```

### 私有披露

- 邮箱：`623272616@qq.com`
- GitHub Security Advisory：[报告漏洞](https://github.com/yyu-wang/fileMind/security/advisories/new)（优先）
- 预期响应时间：**7 天内**

---

## 发布前 checklist

- [ ] 重跑 `cargo test`（Rust 全量）、Python 安全 6 件、前端 consent 4 件，更新自审报告「验证执行记录」
- [ ] 执行自审报告「未自动化项汇总」8 项人工验证并补记录
- [ ] 填入真实仓库地址、commit、私有披露联系方式
- [ ] 确认 `docs/security-self-audit.md` 已纳入仓库（可被评审者直接引用）
