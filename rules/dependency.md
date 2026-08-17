# 依赖管理规范

> 企业级开发要求：依赖版本锁定、定期升级、安全漏洞扫描、新增依赖评审。

## 版本锁定策略

### 三语言依赖锁定

| 语言 | 锁文件 | 规则 |
|------|--------|------|
| TypeScript | `package-lock.json` | 提交到 Git，CI 用 `npm ci` 安装 |
| Rust | `Cargo.lock` | 提交到 Git，CI 用 `--locked` 编译 |
| Python | `requirements.txt` (pinned) | 语义版本精确锁定，不用 `>=` |

### package.json 版本规则

```json
{
  "dependencies": {
    // 精确版本，不用 ^ 或 ~（lockfile 管理实际版本）
    "react": "19.0.0",
    "react-dom": "19.0.0",
    "@tauri-apps/api": "2.1.0"
  }
}
```

### requirements.txt 版本规则

```
# 精确版本锁定（==），不用 >= 或 ~=
fastapi==0.115.0
uvicorn[standard]==0.32.0
pydantic==2.9.0
```

### Cargo.toml 版本规则

```toml
[dependencies]
# 主版本号范围（Cargo.lock 锁定具体版本）
tauri = { version = "2.1", features = ["protocol-asset"] }
serde = { version = "1.0", features = ["derive"] }
```

## 新增依赖评审流程

### 评审清单（新增依赖前必须回答）

- [ ] **必要性**: 现有依赖能否实现该功能？
- [ ] **维护活跃度**: 最近 6 个月有提交吗？GitHub Stars > 100？
- [ ] **安全记录**: npm audit / cargo audit / pip audit 有漏洞吗？
- [ ] **包大小**: 新增依赖的 bundle size 影响多少？
- [ ] **许可证**: MIT / Apache-2.0 / BSD 可接受；GPL 不接受
- [ ] **依赖数**: 该依赖自身有多少传递依赖？（避免依赖爆炸）

### 评审流程

```
需要新依赖 → 填写评审清单 → 自审通过 → 添加到 package.json/Cargo.toml/requirements.txt →
  npm ci / cargo build / pip install → 锁文件更新 → 提交 lockfile
```

## 安全漏洞扫描

### CI 集成

```yaml
# .github/workflows/pr-check.yml 补充
  security-scan:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      # TypeScript
      - uses: actions/setup-node@v4
        with: { node-version: 20, cache: npm }
      - run: npm ci
      - run: npm audit --audit-level=moderate

      # Rust
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo install cargo-audit --locked
      - run: cargo audit --manifest-path src-tauri/Cargo.toml

      # Python
      - uses: actions/setup-python@v5
        with: { python-version: '3.12' }
      - run: pip install pip-audit
      - run: pip-audit -r python-sidecar/requirements.txt
```

### 漏洞处理策略

| 严重级别 | 处理时限 | 处理方式 |
|----------|----------|----------|
| Critical | 立即 | 升级依赖或寻找替代 |
| High | 24 小时内 | 升级依赖 |
| Medium | 本周内 | 评估影响，计划升级 |
| Low | 下个版本 | 记录待办 |

## 定期升级策略

### 升级频率

| 依赖类型 | 频率 | 策略 |
|----------|------|------|
| 框架核心（React/Tauri/Rust） | 每月检查 | 跟进 minor 版本 |
| 工具库（ESLint/Ruff） | 每两周检查 | 跟进 patch 版本 |
| 业务依赖 | 每月检查 | 评估 changelog |
| 安全更新 | 即时 | 立即升级 |

### 升级流程

```
1. npm outdated / cargo update --dry-run / pip list --outdated
2. 逐个升级 minor/patch 版本
3. make lint && make test
4. 手动冒烟测试核心功能
5. 提交 PR + changelog 注明升级内容
```

## 禁止的依赖行为

| 禁止 | 原因 |
|------|------|
| `npm install <pkg>` 不锁版本 | 导致 CI 和本地环境不一致 |
| 使用 `latest` tag | 不可重现的构建 |
| 提交 node_modules/ | 锁文件管理依赖 |
| 修改 lockfile 手动编辑 | 必须通过工具更新 |
| 使用 alpha/beta 版本（非开发期） | 不稳定，影响发布 |
