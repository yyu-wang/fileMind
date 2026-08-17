---
name: "create-store"
description: "Creates a new Zustand store with proper domain separation, persistence strategy, async actions, and test scaffold. Invoke when adding a new state domain to the frontend."
---

# Create Zustand Store

Creates a new Zustand store following `rules/state-management.md` domain separation and persistence standards.

## When to Invoke

- User asks to create a new store
- Adding a new state domain (files, classify, chat, settings, operations, ui)
- Need persistent state with localStorage

## Steps

### 1. Read Rules First

Read `rules/state-management.md` to understand:
- Store domain boundaries
- Persistence strategy per state type
- Data flow patterns
- Event naming conventions

### 2. Determine Store Details

Ask or infer:
- Domain name (e.g., `fileStore`, `chatStore`, `settingsStore`)
- State fields and types
- Actions (sync and async)
- Persistence: none / localStorage / IPC→SQLite
- Event subscriptions (Tauri events to listen)

### 3. Determine Persistence Strategy

| State Type | Persistence | Storage |
|------------|-------------|---------|
| Persistent (file index, rules, logs) | IPC → SQLite | Rust DB |
| Session (chat history, selection) | localStorage | browser |
| Temporary (loading, progress, tokens) | None | memory |
| UI (sidebar, theme) | localStorage | browser |
| Derived (filtered list) | None | selector/useMemo |

### 4. Generate Store File

#### Pattern A: Persistent Store (localStorage)

Write to `src/stores/{domain}Store.ts`:

```tsx
import { create } from 'zustand';
import { persist, createJSONStorage } from 'zustand/middleware';

interface {Domain}State {
  // --- State ---
  {field}: {type};
  // ...

  // --- Actions ---
  {setField}: (value: {type}) => void;
  update{Domain}: (partial: Partial<{Domain}State>) => void;
  reset{Domain}: () => void;
}

const DEFAULTS = {
  {field}: {default_value},
  // ...
};

export const use{Domain}Store = create<{Domain}State>()(
  persist(
    (set) => ({
      ...DEFAULTS,

      {setField}: (value) => set({ {field}: value }),
      update{Domain}: (partial) => set(partial),
      reset{Domain}: () => set(DEFAULTS),
    }),
    {
      name: 'filemind-{domain}',
      storage: createJSONStorage(() => localStorage),
      partialize: (state) => ({
        // Only persist these fields
        {field}: state.{field},
      }),
    }
  )
);
```

#### Pattern B: Async Store (with IPC + Events)

```tsx
import { create } from 'zustand';
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

interface {Domain}State {
  // --- State ---
  {data}: {type}[];
  isLoading: boolean;
  error: string | null;
  progress: { scanned: number; total: number } | null;

  // --- Actions ---
  {fetchAction}: ({params}) => Promise<void>;
  {cancelAction}: () => void;
  clear{Domain}: () => void;
}

export const use{Domain}Store = create<{Domain}State>()((set, get) => {
  let unlisten: UnlistenFn | null = null;

  return {
    {data}: [],
    isLoading: false,
    error: null,
    progress: null,

    {fetchAction}: async ({params}) => {
      set({ isLoading: true, error: null });

      // Subscribe to progress events
      unlisten = await listen('{domain}:progress', (event) => {
        set({ progress: event.payload as { scanned: number; total: number } });
      });

      try {
        const result = await invoke<{returnType}>('{ipc_command}', { {params} });
        set({ {data}: result, progress: null });
      } catch (err) {
        set({ error: err as string });
        throw err;
      } finally {
        unlisten?.();
        unlisten = null;
        set({ isLoading: false });
      }
    },

    {cancelAction}: () => {
      unlisten?.();
      unlisten = null;
      set({ isLoading: false, progress: null });
    },

    clear{Domain}: () => set({ {data}: [], error: null, progress: null }),
  };
});
```

### 5. Store Quality Checklist

- [ ] Domain boundary clear — one store per domain, no cross-store imports
- [ ] State typed with explicit interface (no `any`)
- [ ] Actions are self-contained (don't call other stores)
- [ ] Async actions have loading + error states
- [ ] Event listeners cleaned up in finally block
- [ ] Persistence only for fields that need it (partialize)
- [ ] Store name follows `filemind-{domain}` convention
- [ ] Default values defined in DEFAULTS constant

### 6. Generate Test Scaffold

Write to `src/stores/{domain}Store.test.ts`:

```tsx
import { describe, it, expect, beforeEach } from 'vitest';
import { use{Domain}Store } from './{domain}Store';

describe('{Domain}Store', () => {
  beforeEach(() => {
    use{Domain}Store.setState({ /* defaults */ });
  });

  it('should initialize with default values', () => {
    const state = use{Domain}Store.getState();
    expect(state.{field}).toBe({default_value});
  });

  it('should update field', () => {
    use{Domain}Store.getState().{setField}({new_value});
    expect(use{Domain}Store.getState().{field}).toBe({new_value});
  });

  it('should reset to defaults', () => {
    use{Domain}Store.getState().{setField}({new_value});
    use{Domain}Store.getState().reset{Domain}();
    expect(use{Domain}Store.getState().{field}).toBe({default_value});
  });
});
```

### 7. Self-Check

```bash
npx eslint src/stores/{domain}Store.ts --max-warnings 0
npx tsc --noEmit
npx vitest run src/stores/{domain}Store.test.ts
```

All three MUST pass.
