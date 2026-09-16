import path from 'node:path';
import { existsSync, readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

import js from '@eslint/js';
import tsParser from '@typescript-eslint/parser';
import tsPlugin from '@typescript-eslint/eslint-plugin';
import react from 'eslint-plugin-react';
import reactHooks from 'eslint-plugin-react-hooks';

const CONFIG_DIR = path.dirname(fileURLToPath(import.meta.url));

/** 文件行数阈值（与 rules/complexity.md §文件行数限制、scripts/check-file-size.sh 保持一致）。 */
const FILE_LIMITS = {
  /** React 组件 .tsx */
  component: 300,
  /** React 页面 .tsx */
  page: 400,
  /** TypeScript 工具 .ts */
  ts: 250,
  /** 测试文件 */
  test: 600,
};

/** 圈复杂度强制阈值（rules/complexity.md §函数复杂度限制：警告 10 / 强制 15）。 */
const COMPLEXITY_LIMIT = 15;

/**
 * 读取文件行数基线（scripts/file-size-baseline.txt）。
 *
 * 与 `scripts/check-file-size.sh` 读**同一份**基线而非在 ESLint 里另抄一份：
 * 两套门禁的口径必须一致，否则会出现「脚本说通过、ESLint 说超限」的矛盾。
 * 基线文件缺失时返回空数组（此时标准阈值生效，超限文件会直接报错，问题可见）。
 */
function loadSizeBaseline() {
  const baselineFile = path.join(CONFIG_DIR, 'scripts/file-size-baseline.txt');
  if (!existsSync(baselineFile)) return [];
  return readFileSync(baselineFile, 'utf8')
    .split('\n')
    .map((line) => line.trim())
    .filter((line) => line !== '' && !line.startsWith('#'))
    .map((line) => {
      const [target, max] = line.split(/\s+/);
      return { target, max: Number(max) };
    })
    .filter((entry) => entry.target !== undefined && Number.isFinite(entry.max));
}

const SIZE_BASELINE = loadSizeBaseline()
  // 只挑 .ts/.tsx：其余（.rs/.py/.css）ESLint 不解析，行数由 scripts/check-file-size.sh 管控；
  // 若把 .css 条目也转成 files 模式，ESLint 会把 globals.css 当成待 lint 文件 → 解析报错
  .filter(({ target }) => /\.(?:ts|tsx)$/.test(target));

export default [
  js.configs.recommended,
  {
    files: ['**/*.{ts,tsx}'],
    languageOptions: {
      parser: tsParser,
      parserOptions: {
        ecmaVersion: 2022,
        sourceType: 'module',
        ecmaFeatures: { jsx: true },
      },
      globals: {
        window: 'readonly',
        document: 'readonly',
        console: 'readonly',
        crypto: 'readonly',
        localStorage: 'readonly',
        sessionStorage: 'readonly',
        setTimeout: 'readonly',
        clearTimeout: 'readonly',
        URL: 'readonly',
        Blob: 'readonly',
        File: 'readonly',
        FileReader: 'readonly',
        HTMLElement: 'readonly',
        HTMLDivElement: 'readonly',
        HTMLInputElement: 'readonly',
        Event: 'readonly',
        CustomEvent: 'readonly',
        KeyboardEvent: 'readonly',
        __dirname: 'readonly',
        // vite.config.ts 的 define 注入（package.json version），见 src/vite-env.d.ts
        __APP_VERSION__: 'readonly',
      },
    },
    plugins: {
      '@typescript-eslint': tsPlugin,
      react,
      'react-hooks': reactHooks,
    },
    rules: {
      ...tsPlugin.configs.recommended.rules,
      ...react.configs.recommended.rules,
      ...reactHooks.configs.recommended.rules,

      '@typescript-eslint/no-unused-vars': 'error',
      '@typescript-eslint/no-explicit-any': 'error',
      '@typescript-eslint/consistent-type-imports': 'error',
      '@typescript-eslint/no-non-null-assertion': 'error',

      'react/react-in-jsx-scope': 'off',
      'react/prop-types': 'off',
      'react/display-name': 'error',

      'no-console': ['warn', { allow: ['warn', 'error'] }],
      'no-debugger': 'error',
      'no-duplicate-imports': 'error',
      'no-unreachable': 'error',
      'prefer-const': 'error',

      // 圈复杂度（rules/complexity.md §函数复杂度限制）：超限请拆函数，不要靠
      // 拆条件表达式糊弄——阈值针对的就是分支数量本身
      complexity: ['error', COMPLEXITY_LIMIT],
      // 参数个数 / 嵌套深度 / 嵌套回调（同表）：启用时全仓零违规，纯预防性门禁
      'max-params': ['error', 4],
      'max-depth': ['error', 4],
      'max-nested-callbacks': ['error', 3],
    },
    settings: {
      react: { version: 'detect' },
    },
  },
  // ---- 文件行数（rules/complexity.md §文件行数限制）----
  // 与 scripts/check-file-size.sh 同口径：max-lines 不传 skipBlankLines/skipComments，
  // 计原始行数（等价 `wc -l`），避免「靠多写注释绕过管控」。
  {
    files: ['src/**/*.tsx'],
    rules: { 'max-lines': ['error', FILE_LIMITS.component] },
  },
  {
    // 页面阈值更宽，须排在通用 .tsx 之后——flat config 中后匹配的块覆盖先前的规则值
    files: ['src/pages/**/*.tsx'],
    rules: { 'max-lines': ['error', FILE_LIMITS.page] },
  },
  {
    files: ['src/**/*.ts'],
    rules: { 'max-lines': ['error', FILE_LIMITS.ts] },
  },
  {
    files: ['src/**/*.test.ts', 'src/**/*.test.tsx'],
    rules: { 'max-lines': ['error', FILE_LIMITS.test] },
  },
  // ---- 历史欠账：文件行数 ----
  // 来源 scripts/file-size-baseline.txt（与 scripts/check-file-size.sh 共用同一份）：
  // 允许保持到登记行数，但不得增长；拆分到阈值内后从该文件删行即消账。
  // 须放在上面的分类块之后才算「覆盖」。
  ...SIZE_BASELINE.map(({ target, max }) => ({
    files: [target],
    rules: { 'max-lines': ['error', max] },
  })),
  // ---- 历史欠账：圈复杂度 ----
  // 规则：登记值 = 该文件当前最大复杂度（现状上限，只允许降不允许升）；消账方式是把函数
  // 拆到 15 以内后删除对应行。当前已无登记项，历史欠账全部消账：
  //   - format.ts(22)：getFileTypeMeta 的判断链改为有序规则表，扩展名数据表移入
  //     lib/fileExtensions.ts，函数复杂度回到 3
  //   - RuleForm.tsx(20)：草稿状态与校验收进 useRuleForm，启用开关行拆为 RuleEnabledToggle
  //     组件复杂度回到 4
  //   - CloudProviderFormCard.tsx(30)：状态收进 useCloudProviderForm，校验移入
  //     lib/cloudProviderValidation.ts，五行字段改用 ui/SettingsInputRow，回到 7
  //   - ChatPage.tsx(19)：引用跳转流程收进 hooks/useCitationPreview，头部与输入区拆为
  //     ChatHeader / ChatInputArea，回到 7
  //   - ClassifyPage.tsx(38)：判定收进 lib/classifyView.ts，区域拆为 ClassifyHeader /
  //     ClassifyIntroPanel / ClassifyHistoryView / ClassifyPreviewSection，回到 9
  //   - settingsStore.ts(18) 与 chatStore.ts(16)：字段合并抽成 mergeAppConfig、
  //     事件分发抽成 dispatchChatEvent
  // T9.5 E2E：spec 由 @wdio/globals 注入隐式全局（describe/it/$/browser 等运行时可用，
  // 不需要也不能显式 import；仅声明防止 no-undef 误报）。
  {
    files: ['e2e/**/*.{ts,tsx}'],
    languageOptions: {
      globals: {
        browser: 'readonly',
        $: 'readonly',
        $$: 'readonly',
        describe: 'readonly',
        it: 'readonly',
        before: 'readonly',
        after: 'readonly',
        beforeEach: 'readonly',
        afterEach: 'readonly',
        process: 'readonly', // spec/助手读 env（FILEMIND_* 隔离路径）
      },
    },
  },
  {
    ignores: [
      'node_modules/',
      '.venv/',
      'dist/',
      'src-tauri/',
      'python-sidecar/',
      'src/types/ipc.ts',
      '.bench/',
      // P2-2：Sidecar onedir 产物（gitignore 的构建输出）内含第三方 .js 资源
      // （如 sklearn 的 _repr_html/*.js），非本项目源码，不应参与 lint
      'filemind/binaries/',
    ],
  },
];
