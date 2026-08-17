# TypeScript / React 编码规则

> 来源：06_工程化基础规范.html §3 + AGENTS.md

## 绝对禁止

| 规则 | 说明 | 检查方式 |
|------|------|----------|
| 禁止 `any` 类型 | 所有类型显式标注，需 `any` 需 Code Review 审批 | ESLint `@typescript-eslint/no-explicit-any: error` |
| 禁止默认导出组件 | 统一用具名导出 `export function` | ESLint 规则 + Code Review |
| 禁止 `!` 非空断言 | 使用解构默认值或可选链 | ESLint `@typescript-eslint/no-non-null-assertion: error` |
| 禁止 `console.log` | 只允许 `console.warn` / `console.error` | ESLint `no-console: warn` |
| 禁止手动修改 `src/types/ipc.ts` | 由 tauri-specta 自动生成 | `.gitignore` + CI 检查 |

## 强制要求

### 组件规范
```tsx
// ✅ 正确：Props 接口 + 具名导出 + 解构默认值
interface FileListTableProps {
  files: FileInfo[];
  onFileSelect: (file: FileInfo) => void;
  isLoading?: boolean;
}

export function FileListTable({ files, onFileSelect, isLoading = false }: FileListTableProps) {
  // Hooks 必须在顶层，不得条件调用
  const [selectedId, setSelectedId] = useState<string | null>(null);

  if (isLoading) return <Skeleton />;
  return <div>{/* ... */}</div>;
}
```

### 命名约定
| 类型 | 约定 | 示例 |
|------|------|------|
| 组件 | PascalCase | `FileListTable`, `CitationChip` |
| Hook | camelCase + use 前缀 | `useFileScan`, `useInferenceMode` |
| Store | camelCase + Store 后缀 | `fileStore`, `chatStore` |
| 类型/接口 | PascalCase | `FileInfo`, `ClassifyResult` |
| 枚举 | PascalCase + 值全大写 | `InferenceMode.Local` |
| 常量 | UPPER_SNAKE_CASE | `MAX_FILE_SIZE` |
| 工具函数 | camelCase | `formatFileSize`, `parsePath` |
| 文件名 | 组件 PascalCase，其他 kebab-case | `FileListTable.tsx` / `file-utils.ts` |

### Zustand Store 规范
```tsx
// ✅ 每个 Store 职责单一，需要持久化用 persist
interface FileState {
  files: FileInfo[];
  scanPath: string | null;
  isScanning: boolean;
  scanFiles: (path: string) => Promise<void>;
  clearFiles: () => void;
}

export const useFileStore = create<FileState>()(
  persist(
    (set) => ({
      files: [],
      scanPath: null,
      isScanning: false,
      scanFiles: async (path) => {
        set({ isScanning: true });
        try {
          const files = await ipc.scanDirectory(path);
          set({ files, scanPath: path });
        } finally {
          set({ isScanning: false });
        }
      },
      clearFiles: () => set({ files: [], scanPath: null }),
    }),
    { name: 'filemind-files' }
  )
);
```

### React 组件要点
- **Props 接口**：每个组件必须有 `XxxProps` 接口，放在组件上方
- **key**：列表渲染用唯一稳定 key（文件 id），不用数组 index
- **useEffect**：必须有清理函数（事件监听、订阅）
- **条件渲染**：loading/error/empty 三态分离

### Feature Flag
```tsx
// src/lib/constants/flags.ts 统一管理
export const FEATURE_FLAGS = {
  RAG_ENABLED: false,
  CLOUD_INFERENCE: false,
  RULE_EDITOR: true,
} as const;

// 使用
{FEATURE_FLAGS.RAG_ENABLED && <ChatPage />}
```
