# Code Review 检查清单与 AI 自审流程

> 企业级开发要求：所有代码在提交前必须通过 Code Review，AI 开发的代码必须先自审。

## AI 自审流程（闭合环路）

```
编写代码 → AI 自审（code-review skill）→ 修复问题 → 自检通过（code-self-check skill）→ 
安全审查（security-review skill）→ 提交 PR → CI 自动检查 → 合并
```

### AI 自审触发时机

| 时机 | 审查内容 | 产出 |
|------|----------|------|
| 每个文件编写完成后 | 该文件的规则合规性 | 文件级审查报告 |
| 每个功能完成后 | 跨文件一致性、接口契约 | 功能级审查报告 |
| 提交 PR 前 | 全量自审 | PR 审查报告 |

## 审查维度（12 项）

### 1. 功能正确性
- [ ] 代码实现了需求文档中描述的功能
- [ ] 边界条件已处理（空值、零值、极大值、空列表）
- [ ] 错误路径有测试覆盖
- [ ] 并发场景已考虑（如 Sidecar 重启时的竞态）

### 2. 类型安全
- [ ] TypeScript: 无 `any`，无 `!` 非空断言
- [ ] Rust: 无 `unwrap()`/`expect()`/`panic!()`
- [ ] Python: 无 `Any`，无 `# type: ignore`
- [ ] 所有公共函数有完整类型标注

### 3. 安全合规
- [ ] 文件路径经过 `path_guard::validate()`
- [ ] API Key 通过 Keychain 管理，不硬编码
- [ ] 推理模式切换经过同意校验
- [ ] 云端数据已脱敏
- [ ] 日志不含敏感信息

### 4. 复杂度管控
- [ ] 文件行数在阈值内（TSX < 300, Rust < 500, Python < 500）
- [ ] 函数行数 < 60 行
- [ ] 圈复杂度 < 15
- [ ] 参数个数 < 6
- [ ] 嵌套深度 < 4
- [ ] React 组件 useState < 7, useEffect < 5

### 5. 架构一致性
- [ ] 前端通过 IPC 调用后端，不直接访问文件系统
- [ ] Python Sidecar 不存储 API Key
- [ ] 数据库变更通过 migration 脚本
- [ ] IPC 命令有 `#[specta::specta]` 派生
- [ ] IPC 类型由 `export-specta` 生成

### 6. 错误处理闭环
- [ ] Rust: `Result<T, AppError>` 链式传播，不吞错
- [ ] Python: HTTPException 带正确状态码和用户友好消息
- [ ] 前端: IPC 错误有 toast/inline 提示，不静默失败
- [ ] 错误码遵循错误码体系（见 error-handling.md）

### 7. 测试覆盖
- [ ] 新增功能有对应单元测试
- [ ] 边界条件有测试（空列表、非法路径、权限不足）
- [ ] 测试覆盖率 >= 80%
- [ ] 测试不依赖外部服务（mock Ollama/云 API）

### 8. 性能
- [ ] 大列表使用虚拟滚动（@tanstack/react-virtual）
- [ ] 文件扫描是增量式，不全量重扫
- [ ] Embedding 批量处理，不逐条
- [ ] Rust 热路径无内存分配浪费
- [ ] Python 异步 IO，不阻塞事件循环

### 9. 命名规范
- [ ] 遵循各语言命名约定（见 typescript.md / rust.md / python.md）
- [ ] 变量名有业务含义（`fileList` 而非 `data`）
- [ ] 布尔变量用 is/has/can/should 前缀

### 10. 文档同步
- [ ] 公共函数有 docstring/JSDoc
- [ ] IPC 命令变更已更新 API 规格书
- [ ] 数据库表变更已更新数据模型文档
- [ ] 新增 Feature Flag 已更新 flags.ts

### 11. 依赖管理
- [ ] 新增依赖已评估必要性（是否可用现有依赖替代）
- [ ] 依赖版本锁定（不使用 `^` 或 `~` 在 lockfile 外）
- [ ] 无已知安全漏洞的依赖（`npm audit` / `cargo audit`）

### 12. 提交规范
- [ ] Commit Message 符合 Conventional Commits
- [ ] 一个 PR 对应一个功能/修复（不大杂烩）
- [ ] PR 描述包含变更说明和测试方案
- [ ] CI 全绿

## 自审报告模板

```markdown
## AI Code Review Report

### 审查范围
- 文件: [列出审查的文件]
- 功能: [对应任务 ID]

### 审查结果

| 维度 | 结果 | 问题 |
|------|------|------|
| 功能正确性 | PASS/FAIL | ... |
| 类型安全 | PASS/FAIL | ... |
| 安全合规 | PASS/FAIL | ... |
| 复杂度管控 | PASS/FAIL | ... |
| 架构一致性 | PASS/FAIL | ... |
| 错误处理 | PASS/FAIL | ... |
| 测试覆盖 | PASS/FAIL | ... |
| 性能 | PASS/FAIL | ... |
| 命名规范 | PASS/FAIL | ... |
| 文档同步 | PASS/FAIL | ... |
| 依赖管理 | PASS/FAIL | ... |
| 提交规范 | PASS/FAIL | ... |

### 需修复问题
1. [严重] 文件 xxx.tsx 第 42 行使用了 any 类型
2. [中等] 函数 xxx() 圈复杂度 18，超过阈值 15
3. [建议] 变量名 `d` 可改为 `duration`

### 结论
- [ ] 审查通过，可以提交
- [ ] 需修复 N 个问题后重新审查
```

## 审查严重级别

| 级别 | 说明 | 处理 |
|------|------|------|
| **Blocker** | 安全漏洞、数据丢失风险、架构违规 | 必须修复，禁止提交 |
| **Critical** | 类型安全、错误处理缺失、无测试 | 必须修复 |
| **Major** | 复杂度超标、命名不规范、文档缺失 | 应该修复 |
| **Minor** | 代码风格建议、性能微优化 | 建议修复 |
| **Info** | 信息性提示 | 可忽略 |
