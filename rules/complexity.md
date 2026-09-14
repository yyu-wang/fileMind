# 文件复杂度管控与拆分策略

> 企业级开发要求：每个文件、组件、函数必须有明确的复杂度上限，超限必须拆分。

## 硬性阈值

### 文件行数限制

| 文件类型              | 警告阈值 | 强制拆分阈值 | 检查方式            |
| --------------------- | -------- | ------------ | ------------------- |
| React 组件 (.tsx)     | 200 行   | 300 行       | CI `max-lines` 规则 |
| React 页面 (.tsx)     | 300 行   | 400 行       | CI `max-lines` 规则 |
| Rust 模块 (.rs)       | 300 行   | 500 行       | Clippy + CI 脚本    |
| Python 模块 (.py)     | 300 行   | 500 行       | Ruff + CI 脚本      |
| TypeScript 工具 (.ts) | 150 行   | 250 行       | CI `max-lines` 规则 |
| CSS 文件 (.css)       | 200 行   | 400 行       | CI 脚本             |
| 测试文件              | 400 行   | 600 行       | CI 脚本             |

### 函数复杂度限制

| 指标            | 警告阈值 | 强制重构阈值 | 检查方式                                                      |
| --------------- | -------- | ------------ | ------------------------------------------------------------- |
| 函数行数        | 40 行    | 60 行        | ESLint `max-lines-per-function` / Ruff `PLR0915`              |
| 圈复杂度        | 10       | 15           | ESLint `complexity` / Clippy `cognitive-complexity-threshold` |
| 参数个数        | 4 个     | 6 个         | ESLint `max-params` / Clippy `too-many-arguments-threshold`   |
| 嵌套深度        | 3 层     | 4 层         | ESLint `max-depth`                                            |
| 回调/Promise 链 | 2 层     | 3 层         | Code Review                                                   |

### React 组件复杂度限制

| 指标                     | 警告阈值 | 强制拆分阈值 | 说明                  |
| ------------------------ | -------- | ------------ | --------------------- |
| useState/useReducer 数量 | 5 个     | 7 个         | 超限拆分为自定义 Hook |
| useEffect 数量           | 3 个     | 5 个         | 超限合并或拆分 Hook   |
| props 数量               | 5 个     | 8 个         | 超限合并为对象 props  |
| 条件渲染分支             | 3 个     | 5 个         | 超限拆分子组件        |
| JSX 元素数量             | 50 个    | 80 个        | 超限拆分子组件        |

## 拆分策略

### React 组件拆分决策树

```
组件行数 > 300?
├─ YES → 是否有多个视觉区域?
│   ├─ YES → 按视觉区域拆分为子组件
│   │   例：FileListTable → FileListHeader + FileListBody + FileListFooter
│   └─ NO → 是否有独立逻辑块?
│       ├─ YES → 拆分为子组件 + 自定义 Hook
│       └─ NO → 提取 render 函数（最后手段）
└─ NO → useState > 7?
    ├─ YES → 提取自定义 Hook（useXxxLogic）
    └─ NO → useEffect > 5?
        ├─ YES → 提取自定义 Hook
        └─ NO → 合规，无需拆分
```

### 自定义 Hook 提取规则

```tsx
// ❌ 错误：组件内堆积大量状态逻辑
export function FileListTable({ files }: FileListTableProps) {
  const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set());
  const [sortColumn, setSortColumn] = useState<string>('name');
  const [sortDirection, setSortDirection] = useState<'asc' | 'desc'>('asc');
  const [filterText, setFilterText] = useState('');
  const [page, setPage] = useState(0);
  const [pageSize, setPageSize] = useState(50);
  // ... 60 行状态逻辑
  return <div>{/* JSX */}</div>;
}

// ✅ 正确：提取自定义 Hook
export function FileListTable({ files }: FileListTableProps) {
  const table = useFileTableLogic(files); // 状态 + 逻辑全在 Hook 里
  return <FileTableView {...table} />;
}

// hooks/useFileTableLogic.ts
export function useFileTableLogic(files: FileInfo[]) {
  const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set());
  const [sortColumn, setSortColumn] = useState<string>('name');
  // ... 状态和逻辑
  return { selectedIds, sortedFiles, toggleSelect /* ... */ };
}
```

### Rust 模块拆分规则

```rust
// ❌ 错误：500+ 行的 commands/file_ops.rs
pub async fn scan_directory() { /* ... */ }
pub async fn preview_operations() { /* ... */ }
pub async fn execute_operations() { /* ... */ }
pub async fn undo_batch() { /* ... */ }
// ... 更多函数

// ✅ 正确：按功能拆分模块
// commands/file_ops/mod.rs — 导出
// commands/file_ops/scan.rs — 扫描相关
// commands/file_ops/operations.rs — 操作相关
// commands/file_ops/undo.rs — 撤销相关
```

### Python 模块拆分规则

```python
# ❌ 错误：500+ 行的 services/classifier.py
class Classifier:
    async def classify(self): ...      # 100 行
    async def rule_match(self): ...     # 80 行
    async def llm_fallback(self): ...   # 120 行
    async def confidence_calc(self): ...# 60 行
    async def batch_classify(self): ... # 100 行

# ✅ 正确：按职责拆分
# services/classifier/__init__.py — 导出
# services/classifier/rule_matcher.py — 规则匹配层
# services/classifier/llm_fallback.py — LLM 兜底层
# services/classifier/confidence.py — 置信度计算
# services/classifier/batch.py — 批量处理
```

## CI 强制检查

### 文件行数检查脚本

已落地为 **`scripts/check-file-size.sh`**（CI `frontend-check` 的第一步，也挂在 `make lint`）：

```bash
bash scripts/check-file-size.sh
```

行为（阈值分组与上文「文件行数限制」一一对应）：

| 情形                             | 结果                                       |
| -------------------------------- | ------------------------------------------ |
| 超过**强制拆分阈值**且未登记基线 | **FAIL**（禁止新增超限文件）               |
| 已登记基线，但行数超过基线记录值 | **FAIL**（超限文件只允许拆分，不允许增长） |
| 已登记基线且未增长               | PASS，计入「待消账」清单                   |
| 超过警告阈值但未达强制阈值       | WARN（不阻断）                             |

历史欠账登记在 `scripts/file-size-baseline.txt`（当前 11 个文件，已由 16 消账至 11）；拆分到阈值内后删除对应行即完成消账。
统计口径为**原始行数**（等价 `wc -l`，不剔除空行/注释）；生成物（`src/types/ipc.ts` 由 tauri-specta 生成、禁止手改）与 `node_modules` / `target` / `dist` / `.venv` / `__pycache__` / `gen` 不在管控范围。

### ESLint 复杂度规则

**已启用**（`eslint.config.js`，CI 以 `--max-warnings 0` 运行，故一律按 error 拦截）：

| 规则         | 阈值                                       | 说明                                                                                                                                                                                                |
| ------------ | ------------------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `max-lines`  | 组件 300 / 页面 400 / `.ts` 250 / 测试 600 | 与 `scripts/check-file-size.sh` **同口径**（不跳过空行与注释，计原始行数），并**共用** `scripts/file-size-baseline.txt` 作为历史欠账白名单——两套门禁读同一份基线，避免「脚本说通过、ESLint 说超限」 |
| `complexity` | 15                                         | 函数圈复杂度强制阈值；超限请拆函数                                                                                                                                                                  |

圈复杂度历史欠账（登记在 `eslint.config.js`，登记值 = 该文件当前最大复杂度，**只允许降不允许升**）：

| 文件                                                | 当前上限 | 超限函数（ESLint 报点）      |
| --------------------------------------------------- | -------- | ---------------------------- |
| `src/pages/ClassifyPage.tsx`                        | 38       | `ClassifyPage`               |
| `src/components/settings/CloudProviderFormCard.tsx` | 30       | `CloudProviderFormCard`      |
| `src/lib/format.ts`                                 | 22       | `getFileTypeMeta`            |
| `src/components/rules/RuleForm.tsx`                 | 20       | `RuleForm`                   |
| `src/pages/ChatPage.tsx`                            | 19       | 行 109 的匿名 async 箭头函数 |
| `src/stores/settingsStore.ts`                       | 18       | `updateConfig`               |
| `src/stores/chatStore.ts`                           | 16       | `handleChatEvent`            |

消账方式：把函数拆到 15 以内后，删除 `eslint.config.js` 中对应的 `complexity` 覆盖块。

**本轮已启用**（启用时全仓零违规，属预防性门禁）：`max-params`(4)、`max-depth`(4)、`max-nested-callbacks`(3)。
唯一命中是 `e2e/specs/004-rules.e2e.ts` 里 `describe → it → browser.execute → find` 的四层嵌套，已把浏览器上下文回调提为模块级具名函数（`selectFirstNonEmptyOption`）解决。

**尚未启用：`max-lines-per-function`（60）**——实测数据（2026-09-13，`skipComments: true`）：

| 维度                    | 实测                                                                                                                     |
| ----------------------- | ------------------------------------------------------------------------------------------------------------------------ |
| 违规总量                | **72 处**，散落在 **65 个文件**（生产 41 / 测试 31）                                                                     |
| 生产侧分档              | `>200` 行 7 处、`120–200` 行 10 处、`90–119` 行 10 处、`75–89` 行 6 处、`61–74` 行 8 处                                  |
| 最长函数                | `settingsStore.ts` 283 行、`ClassifyPage.tsx` 267 行、`CloudProviderManager.tsx` 250 行；测试侧最长是单个 `it` 块 308 行 |
| `skipComments` 开关差异 | 关掉后 78 处（仅 +6）——说明这些长函数是实打实的逻辑量，不是注释堆出来的                                                  |

接入路径（建议）：先拆生产侧 7 个 `>200` 行的函数，再带基线开启；测试侧 31 处需另行决定豁免口径——本表的函数行数阈值**没有**测试豁免行（只有文件行数才有 400/600 的测试档）。

需注意：本节的「警告阈值」（函数 40 行 / 复杂度 10 / 参数 4 个）**没有**对应 ESLint 规则——CI 只拦强制阈值，警告档需靠 review 判断。

### Clippy 复杂度规则

```toml
# clippy.toml
cognitive-complexity-threshold = 15
too-many-arguments-threshold = 6
too-many-lines-threshold = 60
type-complexity-threshold = 250
```
