---
name: "component-split"
description: "Analyzes file complexity and splits oversized components/modules into smaller units following the complexity thresholds. Invoke when a file exceeds line limits or a React component has too many hooks/props."
---

# Component Split

Analyzes code complexity and splits oversized files following `rules/complexity.md` thresholds.

## When to Invoke

- File exceeds line limit (TSX > 300, Rust > 500, Python > 500)
- React component has > 7 useState or > 5 useEffect
- Function exceeds 60 lines or cyclomatic complexity > 15
- User asks to "refactor" or "split" a component/file
- CI fails on complexity check

## Steps

### 1. Read Complexity Rules

Read `rules/complexity.md` to understand all thresholds and split strategies.

### 2. Analyze the File

Run complexity analysis on the target file:

```bash
# TypeScript/React
wc -l {file_path}
npx eslint {file_path} --rule '{"max-lines-per-function": ["error", {"max": 60}], "complexity": ["error", 15], "max-params": ["error", 4], "max-depth": ["error", 4]}' --format json

# Rust
wc -l {file_path}
cargo clippy --manifest-path src-tauri/Cargo.toml -- -W clippy::cognitive-complexity -W clippy::too-many-lines

# Python
wc -l {file_path}
ruff check {file_path} --select PLR0915,PLR0911,PLR0912,PLR0913 --exit-zero
```

### 3. Determine Split Strategy

#### React Component Split Decision

```
1. Count lines → > 300?
   YES → Check for multiple visual regions
      → Split by region: Header / Body / Footer / Sidebar
   NO → Continue

2. Count useState/useReducer → > 7?
   YES → Extract custom Hook: use{Feature}Logic
   NO → Continue

3. Count useEffect → > 5?
   YES → Extract custom Hook or merge effects
   NO → Continue

4. Count props → > 8?
   YES → Group related props into objects
   NO → Continue

5. Count JSX elements → > 80?
   YES → Extract sub-components
   NO → File is compliant
```

#### Rust Module Split Decision

```
1. Count lines → > 500?
   YES → Group functions by responsibility
      → Create sub-modules: mod.rs + {sub}.rs files
   NO → Continue

2. Any function > 60 lines?
   YES → Extract helper functions
   NO → Continue

3. Cognitive complexity > 15?
   YES → Extract branches into named helper functions
   NO → File is compliant
```

#### Python Module Split Decision

```
1. Count lines → > 500?
   YES → Group methods by responsibility
      → Create package: __init__.py + {sub}.py files
   NO → Continue

2. Any class > 300 lines?
   YES → Split into multiple classes (SRP)
   NO → Continue

3. Any function > 60 lines?
   YES → Extract helper functions
   NO → File is compliant
```

### 4. Execute Split

#### React Component Split Example

Before (350 lines):
```tsx
// FileListTable.tsx — 350 lines, 8 useState, 4 useEffect
export function FileListTable({ files, onSort, onFilter, onSelect, ...8 props }) {
  const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set());
  const [sortColumn, setSortColumn] = useState('name');
  const [sortDirection, setSortDirection] = useState<'asc'|'desc'>('asc');
  const [filterText, setFilterText] = useState('');
  const [page, setPage] = useState(0);
  const [pageSize, setPageSize] = useState(50);
  const [isLoading, setIsLoading] = useState(false);
  const [error, setError] = useState<string|null>(null);
  // ... 300+ lines of logic + JSX
}
```

After (3 files):
```tsx
// hooks/useFileTableLogic.ts — extracted Hook (~120 lines)
export function useFileTableLogic(files: FileInfo[], initialSort: SortConfig) {
  const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set());
  const [sortColumn, setSortColumn] = useState(initialSort.column);
  // ... all state + logic
  return { selectedIds, sortedFiles, toggleSelect, /* ... */ };
}

// components/file/FileTableHeader.tsx — sub-component (~60 lines)
interface FileTableHeaderProps {
  sortColumn: string;
  sortDirection: 'asc' | 'desc';
  onSort: (column: string) => void;
  filterText: string;
  onFilterChange: (text: string) => void;
}
export function FileTableHeader({ /* ... */ }: FileTableHeaderProps) { /* ... */ }

// components/file/FileListTable.tsx — main component (~80 lines)
export function FileListTable({ files, ...props }: FileListTableProps) {
  const table = useFileTableLogic(files, props.initialSort);
  return (
    <div>
      <FileTableHeader {...table} />
      <FileTableBody files={table.sortedFiles} selectedIds={table.selectedIds} />
      <FileTablePagination page={table.page} total={table.totalPages} onPageChange={table.setPage} />
    </div>
  );
}
```

#### Rust Module Split Example

Before (550 lines in `commands/file_ops.rs`):
After:
```
commands/file_ops/
├── mod.rs           — pub use re-exports (~30 lines)
├── scan.rs          — scan_directory + scan_files (~150 lines)
├── operations.rs    — preview + execute (~200 lines)
└── undo.rs          — undo_batch + undo logic (~150 lines)
```

### 5. Verify Split

```bash
# Check new file sizes are within limits
wc -l {new_files}

# Lint all new files
npx eslint {new_files} --max-warnings 0    # TS
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings  # Rust
ruff check {new_files} && mypy {new_files}  # Python

# Run tests (ensure no regression)
make test
```

### 6. Split Quality Checklist

- [ ] Each new file is within line limit (TSX < 300, Rust < 500, Python < 500)
- [ ] Each function is < 60 lines
- [ ] Cyclomatic complexity < 15 for all functions
- [ ] No circular dependencies between split files
- [ ] All imports updated correctly
- [ ] All tests pass without modification (behavior unchanged)
- [ ] No state/props drilling more than 2 levels deep
- [ ] Custom Hooks are reusable (not tied to one component)
