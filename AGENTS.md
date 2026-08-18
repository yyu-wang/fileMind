# AGENTS.md — AI 开发规则与自检清单

> **此文件是 AI 辅助开发的强制规则手册。任何 AI 代理在本项目中工作前必须完整阅读并遵守。**

## 项目概述

FileMind 是一个桌面文件整理分类器 + RAG 问答系统，基于 Tauri 2 + React 19 + Python FastAPI Sidecar 架构。

- **前端**: `src/` — React 19 + TypeScript strict + Vite + Zustand
- **后端**: `src-tauri/` — Rust + Tauri 2 + SQLite + LanceDB
- **AI 服务**: `python-sidecar/` — Python 3.12 + FastAPI + Ollama

## 规则与技能体系

### Rules（9 个编码规则文件）

AI 在编写任何代码前，必须先阅读对应语言的规则文件：

| 规则文件                    | 适用场景              | 核心内容                                               |
| --------------------------- | --------------------- | ------------------------------------------------------ |
| `rules/typescript.md`       | TS / React 编码       | 组件规范、命名约定、Store 规范、Feature Flag           |
| `rules/rust.md`             | Rust / Tauri 编码     | 错误处理、IPC 命令规范、Clippy 配置                    |
| `rules/python.md`           | Python / FastAPI 编码 | 类型标注、路由规范、异步优先、Pydantic 校验            |
| `rules/security.md`         | 安全红线              | 路径安全、密钥安全、模式切换、数据脱敏、日志脱敏       |
| `rules/sql.md`              | SQL / SQLite          | 命名约定、迁移脚本规范、索引规范                       |
| `rules/complexity.md`       | 复杂度管控            | 文件/函数/组件行数限制、圈复杂度、拆分决策树、CI 检查  |
| `rules/code-review.md`      | Code Review           | 12 维度审查清单、严重级别、AI 自审报告模板             |
| `rules/error-handling.md`   | 错误处理闭环          | 错误码体系（15 个标准码）、Rust/Python/前端错误链路    |
| `rules/state-management.md` | 状态管理              | Store 拆分边界、持久化策略、数据流闭环、事件命名       |
| `rules/dependency.md`       | 依赖管理              | 版本锁定、新增依赖评审、安全漏洞扫描、升级策略         |
| `rules/performance.md`      | 性能预算              | 前端/Rust/Python 性能指标、虚拟滚动、批量操作、CI 门控 |
| `rules/observability.md`    | 可观测性              | 日志分级、脱敏规则、指标埋点、链路追踪、健康检查       |
| `rules/release.md`          | 发布流程              | 版本号策略、Changelog、发布检查清单、回滚机制          |

### Skills（10 个开发技能）

AI 在执行以下操作时，必须调用对应的 Skill：

| Skill                    | 触发场景               | 产出                                            |
| ------------------------ | ---------------------- | ----------------------------------------------- |
| `create-ipc-command`     | 创建新 Tauri IPC 命令  | Rust 命令 + 路径校验 + specta + 测试 + 类型生成 |
| `create-react-component` | 创建新 React 组件      | 组件 + Props 接口 + 测试骨架                    |
| `create-python-route`    | 创建新 FastAPI 路由    | Pydantic 模型 + 路由 + docstring + 测试         |
| `create-store`           | 创建新 Zustand Store   | Store + 持久化策略 + 异步 action + 测试         |
| `create-error-code`      | 创建新错误码           | 错误码注册 + Rust/Python/前端错误链路           |
| `create-migration`       | 创建数据库迁移         | SQL 脚本 + 命名校验 + FTS5 触发器 + 模型更新    |
| `component-split`        | 文件/组件超阈值        | 复杂度分析 + 拆分决策 + 执行拆分 + 验证         |
| `code-review`            | 完成功能后、提交前     | 12 维度审查 + 自动化检查 + 审查报告             |
| `code-self-check`        | 完成代码后、提交前     | 三语言全量 lint + test + security scan          |
| `security-review`        | 涉及路径/密钥/模式切换 | 7 项安全红线逐项审查 + 扫描命令 + 报告          |

## 架构约束

### 绝对禁止

1. **禁止 `unwrap()` / `expect()` / `panic!()`** — Rust 生产代码中完全禁止，所有可恢复错误用 `Result<T, E>` 返回
2. **禁止 `any` 类型** — TypeScript 和 Python 中均禁止，所有类型显式标注
3. **禁止默认导出** — React 组件统一用具名导出（`export function`）
4. **禁止手动修改 `src/types/ipc.ts`** — 该文件由 tauri-specta 自动生成
5. **禁止在 Python Sidecar 中存储 API Key** — 密钥通过 Rust Keychain 管理后转发
6. **禁止跳过路径校验** — 所有文件路径参数必须经过 `path_guard::validate()`
7. **禁止未签名的 HTTP** — 云端请求必须通过 Rust 代理转发，不直接从 Python 发起

### 强制要求

1. **所有 IPC 命令** 必须加 `#[specta::specta]` 派生宏，返回 `Result<T, String>`
2. **所有 Python 函数** 必须有类型标注（mypy strict）和 Google 风格 docstring
3. **所有 React 组件** 必须有 `XxxProps` 接口定义，放在组件上方
4. **所有数据库变更** 必须通过 refinery 迁移脚本，不直接修改表结构
5. **所有 Commit Message** 必须符合 Conventional Commits（`feat(scope): desc`）
6. **所有 PR** 必须通过 CI 全绿才能合并

## 开发流程（AI 必须遵守 — 闭合环路）

### 1. 编码前

```
确认任务 ID（如 T3.1）→ 查看依赖任务是否完成 → 阅读对应 rules/{language}.md + rules/complexity.md →
  调用对应 Skill（create-ipc-command / create-react-component / create-python-route / create-store / create-migration / create-error-code）→ 开始编码
```

### 2. 编码中

- **TypeScript**: ESLint + Prettier 规则自动执行，不得 disable 规则
- **Rust**: clippy `all = "deny"`，不得使用 `#[allow(...)]` 绕过
- **Python**: ruff + mypy strict，不得使用 `# type: ignore` 绕过
- **复杂度管控**: 文件行数实时监控（TSX < 300, Rust < 500, Python < 500），超限调用 `component-split` skill
- **错误处理**: 新增错误场景调用 `create-error-code` skill
- **状态管理**: 新增 Store 调用 `create-store` skill

### 3. 编码后自检（必须全部通过才能提交）

调用 `code-self-check` skill 执行全量自检，或手动执行：

```bash
make lint    # 三语言全量检查
make test    # 三语言全量测试
```

自检清单：

- [ ] ESLint 零 warnings（`--max-warnings 0`）
- [ ] Rust clippy 零 warnings（`-D warnings`）
- [ ] Python ruff + mypy 零 errors
- [ ] 所有新增函数有类型标注
- [ ] 所有新增 IPC 命令有 `#[specta::specta]`
- [ ] 所有新增文件路径操作经过 `path_guard`
- [ ] 无 `console.log` / `println!` / `print()` 残留（用 logger）
- [ ] 无 `unwrap()` / `expect()` / `any` 类型
- [ ] 新增功能有对应测试
- [ ] 文件行数在阈值内（调用 `component-split` 如果超限）
- [ ] 函数行数 < 60，圈复杂度 < 15

### 4. 安全审查（涉及安全敏感代码时）

如果代码涉及文件路径、API Key、推理模式切换，必须调用 `security-review` skill 执行安全审查。

### 5. Code Review（提交 PR 前）

调用 `code-review` skill 执行 12 维度全量审查：

- 功能正确性、类型安全、安全合规、复杂度管控
- 架构一致性、错误处理、测试覆盖、性能
- 命名规范、文档同步、依赖管理、提交规范
- 审查报告生成后，所有 Blocker/Critical 问题必须修复才能提交。

### 6. 提交前

```bash
make format   # 自动格式化
make gen:ipc  # 如果修改了 IPC 命令，重新生成类型
```

### 7. 提交后

```
CI 自动检查（pr-check.yml）→ 全绿 → 合并 → merge-build.yml 三平台构建 → 发布时按 rules/release.md 流程
```

### 闭合环路图

```
任务确认 → 阅读 rules → 调用 create-* skill 生成代码 → 编码（遵守 complexity 规则）→
  code-self-check（lint+test）→ security-review（安全审查）→ code-review（12 维度审查）→
  修复问题 → format + gen:ipc → 提交 PR → CI 全绿 → 合并 → 发布
```

## 文件修改规则

### 可以直接修改

- `src/components/` — React 组件
- `src/hooks/` — 自定义 Hooks
- `src/stores/` — Zustand stores
- `src/pages/` — 页面组件
- `src/lib/` — 工具函数
- `src-tauri/src/commands/` — IPC 命令实现
- `src-tauri/src/security/` — 安全模块
- `python-sidecar/app/` — Python 业务逻辑

### 修改前需要确认

- `package.json` — 依赖变更需确认必要性
- `Cargo.toml` — 依赖变更需确认必要性
- `tsconfig.json` / `eslint.config.js` — 规则变更需确认影响范围
- `src-tauri/tauri.conf.json` — 配置变更需确认安全影响
- `.github/workflows/` — CI 变更需确认不破坏流水线

### 禁止直接修改

- `src/types/ipc.ts` — 由 tauri-specta 生成
- `src-tauri/Cargo.lock` / `package-lock.json` — 由依赖管理工具维护

## 安全红线

### 路径安全

```rust
// ✅ 正确：所有路径参数经过校验
let safe_path = path_guard::validate(&path)?;

// ❌ 错误：直接使用用户输入的路径
let files = std::fs::read_dir(&path)?;
```

### 密钥安全

```rust
// ✅ 正确：密钥存储在 Keychain，通过 Rust 转发
let key = keychain::get_key("openai")?;
let response = reqwest::post(url).header("Authorization", format!("Bearer {key}")).send().await?;

// ❌ 错误：密钥硬编码或传给 Python
const API_KEY = "sk-...";  // 绝对禁止
```

### 模式切换安全

```rust
// ✅ 正确：模式切换经过同意校验
security::mode_switch::validate_mode_switch("local", "cloud", &source)?;

// ❌ 错误：直接切换不校验
set_mode("cloud");  // 绝对禁止
```

## 代码生成规则

### IPC 命令模板

```rust
#[tauri::command]
#[specta::specta]
pub async fn command_name(param: Type) -> Result<ReturnType, String> {
    let safe_value = validate(&param).map_err(|e| e.to_string())?;
    // 业务逻辑
    Ok(result)
}
```

### React 组件模板

```tsx
interface ComponentNameProps {
  prop1: string;
  onAction: (value: string) => void;
  optional?: boolean;
}

export function ComponentName({ prop1, onAction, optional = false }: ComponentNameProps) {
  // hooks 在顶层
  // 渲染逻辑
  return <div>...</div>;
}
```

### Python 路由模板

```python
@router.post("", response_model=ResponseType)
async def endpoint(request: RequestType) -> ResponseType:
    """简述功能。

    Args:
        request: 请求体说明

    Returns:
        返回值说明

    Raises:
        HTTPException 400: 参数错误
        HTTPException 500: 内部错误
    """
    # 业务逻辑
    return ResponseType(...)
```

## 任务推进规则

1. 按任务拆解文档（10_开发任务拆解与排期.html）中的顺序推进
2. 每个任务完成后勾选检查清单
3. 不跳过依赖任务（如 T3.1 依赖 T0.7 和 T2.3）
4. 门控任务（E1/E5/E11）必须全部通过才能进入下一 Epic
5. 每次提交对应一个任务，Commit Message 包含任务 ID

## 参考文档

- PRD: `项目开发文件/01_产品需求文档PRD.html`
- 实施计划: `项目开发文件/02_总体实施计划.html`
- API 规格: `项目开发文件/04_API详细规格书.html`
- 工程规范: `项目开发文件/06_工程化基础规范.html`
- 安全合规: `项目开发文件/07_安全合规设计.html`
- Prompt 设计: `项目开发文件/08_Prompt工程设计.html`
- 测试体系: `项目开发文件/09_测试体系设计.html`
- 任务拆解: `项目开发文件/10_开发任务拆解与排期.html`
