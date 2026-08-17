# 文件复杂度管控与拆分策略

> 企业级开发要求：每个文件、组件、函数必须有明确的复杂度上限，超限必须拆分。

## 硬性阈值

### 文件行数限制

| 文件类型 | 警告阈值 | 强制拆分阈值 | 检查方式 |
|----------|----------|--------------|----------|
| React 组件 (.tsx) | 200 行 | 300 行 | CI `max-lines` 规则 |
| React 页面 (.tsx) | 300 行 | 400 行 | CI `max-lines` 规则 |
| Rust 模块 (.rs) | 300 行 | 500 行 | Clippy + CI 脚本 |
| Python 模块 (.py) | 300 行 | 500 行 | Ruff + CI 脚本 |
| TypeScript 工具 (.ts) | 150 行 | 250 行 | CI `max-lines` 规则 |
| CSS 文件 (.css) | 200 行 | 400 行 | CI 脚本 |
| 测试文件 | 400 行 | 600 行 | CI 脚本 |

### 函数复杂度限制

| 指标 | 警告阈值 | 强制重构阈值 | 检查方式 |
|------|----------|--------------|----------|
| 函数行数 | 40 行 | 60 行 | ESLint `max-lines-per-function` / Ruff `PLR0915` |
| 圈复杂度 | 10 | 15 | ESLint `complexity` / Clippy `cognitive-complexity-threshold` |
| 参数个数 | 4 个 | 6 个 | ESLint `max-params` / Clippy `too-many-arguments-threshold` |
| 嵌套深度 | 3 层 | 4 层 | ESLint `max-depth` |
| 回调/Promise 链 | 2 层 | 3 层 | Code Review |

### React 组件复杂度限制

| 指标 | 警告阈值 | 强制拆分阈值 | 说明 |
|------|----------|--------------|------|
| useState/useReducer 数量 | 5 个 | 7 个 | 超限拆分为自定义 Hook |
| useEffect 数量 | 3 个 | 5 个 | 超限合并或拆分 Hook |
| props 数量 | 5 个 | 8 个 | 超限合并为对象 props |
| 条件渲染分支 | 3 个 | 5 个 | 超限拆分子组件 |
| JSX 元素数量 | 50 个 | 80 个 | 超限拆分子组件 |

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
  const table = useFileTableLogic(files);  // 状态 + 逻辑全在 Hook 里
  return <FileTableView {...table} />;
}

// hooks/useFileTableLogic.ts
export function useFileTableLogic(files: FileInfo[]) {
  const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set());
  const [sortColumn, setSortColumn] = useState<string>('name');
  // ... 状态和逻辑
  return { selectedIds, sortedFiles, toggleSelect, /* ... */ };
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
```bash
# scripts/check-file-size.sh
MAX_TSX=300; MAX_TS=250; MAX_RS=500; MAX_PY=500

find src -name "*.tsx" -exec wc -l {} + | awk -v max=$MAX_TSX '
  $1 > max && $2 != "total" { print "FAIL: " $2 " has " $1 " lines (max " max ")" }
'
find src -name "*.ts" -exec wc -l {} + | awk -v max=$MAX_TS '
  $1 > max && $2 != "total" { print "FAIL: " $2 " has " $1 " lines (max " max ")" }
'
find src-tauri/src -name "*.rs" -exec wc -l {} + | awk -v max=$MAX_RS '
  $1 > max && $2 != "total" { print "FAIL: " $2 " has " $1 " lines (max " max ")" }
'
find python-sidecar/app -name "*.py" -exec wc -l {} + | awk -v max=$MAX_PY '
  $1 > max && $2 != "total" { print "FAIL: " $2 " has " $1 " lines (max " max ")" }
'
```

### ESLint 复杂度规则
```javascript
// eslint.config.js 补充
rules: {
  'max-lines-per-function': ['error', { max: 60, skipComments: true }],
  'max-lines': ['warn', { max: 300, skipBlankLines: true, skipComments: true }],
  'complexity': ['error', 15],
  'max-params': ['error', 4],
  'max-depth': ['error', 4],
  'max-nested-callbacks': ['error', 3],
}
```

### Clippy 复杂度规则
```toml
# clippy.toml
cognitive-complexity-threshold = 15
too-many-arguments-threshold = 6
too-many-lines-threshold = 60
type-complexity-threshold = 250
```
