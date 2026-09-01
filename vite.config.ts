import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import path from 'path';

export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      '@': path.resolve(import.meta.dirname, './src'),
      '@components': path.resolve(import.meta.dirname, './src/components'),
      '@hooks': path.resolve(import.meta.dirname, './src/hooks'),
      '@stores': path.resolve(import.meta.dirname, './src/stores'),
      '@types': path.resolve(import.meta.dirname, './src/types'),
      '@lib': path.resolve(import.meta.dirname, './src/lib'),
    },
  },
  // pdf.js 体积大且含 worker，跳过预打包避免构建慢/双实例
  optimizeDeps: {
    exclude: ['pdfjs-dist'],
  },
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
  },
  build: {
    target: 'es2022',
    minify: 'esbuild',
    sourcemap: false,
    rollupOptions: {
      output: {
        // 稳定 vendor 拆分（rolldown-vite 仅支持函数形式）：react 系框架代码
        // 变化少、利于长缓存；react-pdf/pdfjs 刻意不匹配，由 LazyFilePreviewDrawer
        // 的动态导入边界拆为异步 chunk
        manualChunks(id: string): string | undefined {
          if (!id.includes('node_modules')) return undefined;
          if (
            /node_modules\/(react|react-dom|react-router|react-router-dom|scheduler|zustand|@tanstack\/react-virtual|use-sync-external-store)\//.test(
              id,
            )
          ) {
            return 'vendor-react';
          }
          return undefined;
        },
      },
    },
  },
  test: {
    globals: true,
    environment: 'jsdom',
    setupFiles: ['./tests/setup.ts'],
    coverage: {
      provider: 'v8',
      reporter: ['text', 'lcov'],
      // 自动生成（specta）与 IPC 薄封装不参与覆盖率：业务价值低且依赖运行环境
      exclude: ['src/types/ipc.ts', 'src/lib/ipc/**'],
      thresholds: {
        lines: 80,
        functions: 80,
        branches: 75,
        statements: 80,
      },
    },
  },
});
