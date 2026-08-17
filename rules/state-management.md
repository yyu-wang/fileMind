# 状态管理规范

> 企业级开发要求：状态拆分有边界、持久化有策略、数据流向清晰可追踪。

## Store 拆分原则

### 按领域拆分（不按页面拆分）

```
stores/
├── fileStore.ts        — 文件列表、扫描状态、选中状态
├── classifyStore.ts    — 分类结果、分类规则、置信度
├── chatStore.ts        — 对话历史、RAG 引用、流式状态
├── settingsStore.ts    — 用户配置、推理模式、模型选择
├── operationStore.ts   — 操作日志、批量操作、撤销栈
└── uiStore.ts          — 侧边栏折叠、主题、通知
```

### Store 边界规则

| 规则 | 说明 |
|------|------|
| 单一职责 | 每个 Store 只管一个领域，不跨域 |
| 不嵌套 Store | Store 之间不互相 import，通过组件层组合 |
| 只存共享状态 | 组件私有状态用 useState，不上提到 Store |
| 派生状态用 selector | 不在 Store 中存可计算的数据 |

### 状态分类与持久化策略

| 状态类型 | 示例 | 持久化 | 存储位置 |
|----------|------|--------|----------|
| **持久状态** | 文件索引、分类规则、操作日志 | 是 | SQLite（通过 IPC） |
| **会话状态** | 对话历史、当前选中文件 | 会话级 | localStorage |
| **临时状态** | 加载中、扫描进度、流式 token | 否 | 内存（Zustand） |
| **UI 状态** | 侧边栏折叠、主题模式 | 是 | localStorage |
| **派生状态** | 过滤后文件列表、排序后列表 | 否 | selector / useMemo |

## Zustand Store 模板

### 标准 Store（带持久化）

```tsx
// stores/settingsStore.ts
import { create } from 'zustand';
import { persist, createJSONStorage } from 'zustand/middleware';

interface SettingsState {
  // --- 状态 ---
  inferenceMode: 'local' | 'cloud';
  embeddingModel: string;
  maxFileSizeMb: number;
  language: 'zh-CN' | 'en-US';

  // --- 操作 ---
  setInferenceMode: (mode: 'local' | 'cloud') => void;
  setEmbeddingModel: (model: string) => void;
  updateSettings: (partial: Partial<Omit<SettingsState, 'setInferenceMode' | 'setEmbeddingModel' | 'updateSettings' | 'resetSettings'>>) => void;
  resetSettings: () => void;
}

const DEFAULT_SETTINGS = {
  inferenceMode: 'local' as const,
  embeddingModel: 'bge-large-zh-v1.5',
  maxFileSizeMb: 100,
  language: 'zh-CN' as const,
};

export const useSettingsStore = create<SettingsState>()(
  persist(
    (set) => ({
      ...DEFAULT_SETTINGS,

      setInferenceMode: (mode) => set({ inferenceMode: mode }),
      setEmbeddingModel: (model) => set({ embeddingModel: model }),
      updateSettings: (partial) => set(partial),
      resetSettings: () => set(DEFAULT_SETTINGS),
    }),
    {
      name: 'filemind-settings',
      storage: createJSONStorage(() => localStorage),
      // 只持久化部分字段
      partialize: (state) => ({
        inferenceMode: state.inferenceMode,
        embeddingModel: state.embeddingModel,
        maxFileSizeMb: state.maxFileSizeMb,
        language: state.language,
      }),
    }
  )
);
```

### 异步 Store（带 IPC 调用）

```tsx
// stores/fileStore.ts
import { create } from 'zustand';
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

interface FileState {
  // --- 状态 ---
  files: FileInfo[];
  scanPath: string | null;
  isScanning: boolean;
  scanProgress: { scanned: number; total: number } | null;
  selectedIds: Set<string>;

  // --- 操作 ---
  scanFiles: (path: string) => Promise<void>;
  cancelScan: () => void;
  selectFile: (id: string) => void;
  toggleSelect: (id: string) => void;
  clearSelection: () => void;
  clearFiles: () => void;
}

export const useFileStore = create<FileState>()((set, get) => {
  let unlistenProgress: UnlistenFn | null = null;

  return {
    files: [],
    scanPath: null,
    isScanning: false,
    scanProgress: null,
    selectedIds: new Set(),

    scanFiles: async (path) => {
      set({ isScanning: true, scanProgress: { scanned: 0, total: 0 } });

      // 监听扫描进度事件
      unlistenProgress = await listen('scan:progress', (event) => {
        set({ scanProgress: event.payload as { scanned: number; total: number } });
      });

      try {
        const files = await invoke<FileInfo[]>('scan_directory', { path });
        set({ files, scanPath: path });
      } catch (error) {
        // 错误交给调用方处理
        throw error;
      } finally {
        unlistenProgress?.();
        unlistenProgress = null;
        set({ isScanning: false, scanProgress: null });
      }
    },

    cancelScan: () => {
      unlistenProgress?.();
      unlistenProgress = null;
      set({ isScanning: false, scanProgress: null });
    },

    selectFile: (id) => set({ selectedIds: new Set([id]) }),

    toggleSelect: (id) => {
      const current = get().selectedIds;
      const next = new Set(current);
      if (next.has(id)) {
        next.delete(id);
      } else {
        next.add(id);
      }
      set({ selectedIds: next });
    },

    clearSelection: () => set({ selectedIds: new Set() }),
    clearFiles: () => set({ files: [], scanPath: null, selectedIds: new Set() }),
  };
});
```

## 数据流闭环

### 前端 → IPC → Rust → Python → 返回

```
用户操作 → Store action → invoke IPC → Rust command →
  ├─ path_guard 校验
  ├─ DB 操作（refinery migration）
  ├─ Sidecar proxy（HTTP → Python FastAPI）
  │    ├─ Pydantic 校验
  │    ├─ 业务逻辑（Ollama/LanceDB）
  │    └─ 返回结果 / 抛 FileMindError
  └─ 返回 Result<T, String> → 前端 Store 更新 → UI 渲染
```

### 事件流（Rust → 前端）

```
Rust 事件发射 → tauri event → 前端 listen → Store 更新 → UI 渲染

事件命名: {domain}:{action}
  scan:progress      — 扫描进度
  scan:complete      — 扫描完成
  classify:progress  — 分类进度
  classify:complete  — 分类完成
  chat:token         — 流式 token
  sidecar:status     — Sidecar 状态变更
```

## 状态同步规则

| 场景 | 策略 |
|------|------|
| 文件列表变更 | Store 更新 + DB 异步写入（不阻塞 UI） |
| 分类结果更新 | Store 更新 + DB upsert |
| 操作日志 | 先写 DB（链式哈希），再更新 Store |
| 设置变更 | Store 更新 + localStorage 持久化 + IPC 通知 Rust |
| 多窗口同步 | 通过 Tauri event 广播，各窗口 Store 监听更新 |
