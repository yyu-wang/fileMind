# 发布流程规范

> 企业级开发要求：版本号策略、changelog 管理、回滚机制、发布检查清单。

## 版本号策略

### 语义化版本 (SemVer)

```
MAJOR.MINOR.PATCH
  1.0.0

MAJOR: 不兼容的 API 变更（如 IPC 接口重构）
MINOR: 向下兼容的新功能（如新增分类规则编辑器）
PATCH: 向下兼容的 Bug 修复
```

### 发布阶段

| 阶段   | 版本号          | 说明                 | 门控                     |
| ------ | --------------- | -------------------- | ------------------------ |
| Alpha  | `1.1.0-alpha.N` | 内部测试，功能不完整 | 无                       |
| Beta   | `1.1.0-beta.N`  | 功能完整，可能有 Bug | E1/E5 门控通过           |
| RC     | `1.1.0-rc.N`    | 候选发布，仅修 Bug   | E11 门控通过             |
| Stable | `1.1.0`         | 正式发布             | 全量测试通过 + 无 P0 Bug |

### 版本号变更规则

| 变更类型     | 版本号变更 | 示例              |
| ------------ | ---------- | ----------------- |
| 新 Epic 完成 | MINOR +1   | `1.0.0` → `1.1.0` |
| Bug 修复     | PATCH +1   | `1.0.0` → `1.0.1` |
| IPC 接口重构 | MAJOR +1   | `1.0.0` → `2.0.0` |
| 预发布       | 后缀       | `1.1.0-beta.1`    |

## Changelog 管理

### 格式（Keep a Changelog）

```markdown
# Changelog

## [Unreleased]

## [1.1.0] - 2026-09-15

### Added

- 三层分类引擎（规则 → 关键词 → LLM 兜底）
- RAG 问答系统（流式输出 + 引用标注）
- 分类规则编辑器

### Changed

- 文件扫描改为增量模式（全量扫描耗时降低 60%）
- Sidecar 启动流程优化（冷启动 < 2s）

### Fixed

- #42 大量文件时 UI 卡顿（添加虚拟滚动）
- #51 macOS 下路径校验误报

### Security

- 修复路径遍历漏洞（SEC-003）
- API Key 存储迁移到 OS Keychain
```

### Changelog 规则

- 每个 PR 必须在 `[Unreleased]` 下添加变更条目
- 条目格式：`- #{issue_number} {描述}`
- 分类：Added / Changed / Deprecated / Removed / Fixed / Security
- 发布时将 `[Unreleased]` 改为 `[版本号] - 日期`

## 发布检查清单

### 发布前必检（E11 门控）

| 检查项           | 命令/方式               | 通过标准           |
| ---------------- | ----------------------- | ------------------ |
| 三语言 lint      | `make lint`             | 零 warnings        |
| 三语言测试       | `make test`             | 全部通过           |
| 覆盖率           | `make test:coverage`    | >= 80%             |
| 安全自审         | `security-review` skill | 零 Blocker         |
| Bundle size      | CI performance-check    | JS < 200KB         |
| 三平台构建       | CI merge-build          | 4 目标全过         |
| macOS 签名+公证  | 手动验证                | Gatekeeper 无拦截  |
| Windows 签名     | 手动验证                | SmartScreen 可绕过 |
| 数据库迁移       | `cargo test db::`       | 全部通过           |
| Sidecar 健康检查 | 手动验证                | /health 返回 ok    |
| 无 P0 Bug        | GitHub Issues           | 零 P0              |
| Changelog 更新   | 手动检查                | 所有变更已记录     |

### 发布流程

```
1. 创建 release 分支: git checkout -b release/1.1.0
2. 更新版本号:
   - package.json: "version": "1.1.0"
   - src-tauri/Cargo.toml: version = "1.1.0"
   - src-tauri/tauri.conf.json: "version": "1.1.0"
   - python-sidecar/app/main.py: version="1.1.0"
3. 更新 Changelog: [Unreleased] → [1.1.0] - YYYY-MM-DD
4. 全量测试: make test
5. 三平台构建: make build
6. 签名: macOS 公证 + Windows OV 签名
7. 创建 GitHub Release: tag v1.1.0 + 上传安装包
8. 合并 release 分支到 main
```

## 回滚机制

### 回滚策略

| 场景               | 回滚方式                                         | 耗时      |
| ------------------ | ------------------------------------------------ | --------- |
| 发布后发现 Bug     | GitHub Release 标记为 pre-release + 发布上一版本 | < 10 分钟 |
| 数据库迁移问题     | 新建回滚 migration（不删除已迁移数据）           | < 30 分钟 |
| Sidecar 版本不兼容 | Tauri 配置回退到上一版本 Sidecar 二进制          | < 5 分钟  |

### 回滚检查清单

- [ ] 确认回滚原因和影响范围
- [ ] 确认回滚目标版本号
- [ ] 确认数据库兼容性（是否需要回滚 migration）
- [ ] 通知已升级用户（通过应用内通知）
- [ ] 创建回滚 PR + Release
- [ ] Post-mortem 文档（根因分析 + 预防措施）

### 数据库迁移回滚

```sql
-- V006__rollback_v005.sql
-- 回滚 V005 的 FTS5 表（不删除 files 表数据）

DROP TRIGGER IF EXISTS files_ai;
DROP TRIGGER IF EXISTS files_ad;
DROP TRIGGER IF EXISTS files_au;
DROP TABLE IF EXISTS file_fts;
```

## Git Tag 规范

```bash
# 创建 tag
git tag -a v1.1.0 -m "Release 1.1.0: 三层分类引擎 + RAG 问答"

# 推送 tag
git push origin v1.1.0

# Tag 命名
v{MAJOR}.{MINOR}.{PATCH}[-{stage}.{N}]
  v1.0.0
  v1.1.0-beta.1
  v1.1.0-rc.1
  v1.1.0
```
