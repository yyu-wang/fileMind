---
name: "create-react-component"
description: "Creates a new React component with Props interface, named export, test scaffold, and styling. Invoke when adding a new UI component to the frontend."
---

# Create React Component

Creates a new React component following project TypeScript strict and naming conventions.

## When to Invoke

- User asks to create a new React component
- User asks to add a new UI element
- Creating a new page or widget

## Steps

### 1. Read Rules First

Before generating any code, read:
- `rules/typescript.md` — TypeScript/React coding rules

### 2. Determine Component Details

Ask or infer:
- Component name (PascalCase, e.g. `FileListTable`)
- Category: `ui/` | `file/` | `classify/` | `chat/` | `settings/` | `common/`
- Props (required and optional)
- Whether it needs state (useState/useReducer)
- Whether it needs hooks (custom hooks from `hooks/`)
- Whether it connects to a Zustand store

### 3. Generate Component File

Write to `src/components/{category}/{ComponentName}.tsx`:

```tsx
import { useState } from 'react';

interface {ComponentName}Props {
  // Required props (no optional marker)
  // Optional props with ? marker
  isLoading?: boolean;
}

export function {ComponentName}({ isLoading = false }: {ComponentName}Props) {
  // Hooks at top level — never conditional
  const [state, setState] = useState<null>(null);

  // Early returns for loading/error states
  if (isLoading) return <div>Loading...</div>;

  // Main render
  return (
    <div className="{component-name}">
      {/* Component content */}
    </div>
  );
}
```

### 4. Code Quality Checklist (MUST verify)

- [ ] Props interface named `{ComponentName}Props`, placed above component
- [ ] Named export (`export function`), NOT default export
- [ ] No `any` type — all props typed
- [ ] No `!` non-null assertion — use optional chaining or defaults
- [ ] Optional props have destructured default values
- [ ] Hooks called at top level (not in conditions/loops)
- [ ] List rendering uses unique stable key (not array index)
- [ ] useEffect has cleanup function if it sets up listeners/subscriptions
- [ ] No `console.log` — use `console.warn` or `console.error` if needed

### 5. Generate Test Scaffold

Write to `src/components/{category}/{ComponentName}.test.tsx`:

```tsx
import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/react';
import { {ComponentName} } from './{ComponentName}';

describe('{ComponentName}', () => {
  it('should render without crashing', () => {
    render(<{ComponentName} />);
    expect(screen.getByText(/.*/)).toBeInTheDocument();
  });

  it('should show loading state', () => {
    render(<{ComponentName} isLoading={true} />);
    expect(screen.getByText(/loading/i)).toBeInTheDocument();
  });
});
```

### 6. If Store Connection Needed

If component connects to a Zustand store:

```tsx
import { useFileStore } from '@stores/fileStore';

export function {ComponentName}() {
  const { files, scanFiles } = useFileStore();
  // Use store state and actions
}
```

### 7. Self-Check

```bash
npx eslint src/components/{category}/{ComponentName}.tsx --max-warnings 0
npx tsc --noEmit
npx vitest run src/components/{category}/{ComponentName}.test.tsx
```

All three MUST pass with zero warnings.
