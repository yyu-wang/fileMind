import { Suspense, lazy } from 'react';
import { HashRouter, Routes, Route } from 'react-router-dom';
import { AppLayout } from './components/layout/AppLayout';
import { OnboardingWizard } from './components/onboarding/OnboardingWizard';
import { ToastContainer } from './components/ui/Toast';
import { useSettingsStore } from './stores/settingsStore';

// 路由级代码分割：每个页面独立异步 chunk，首屏只加载当前页。
// 项目规则禁止默认导出，故用 then 包装把具名导出映射为 default。
const FilesPage = lazy(() => import('./pages/FilesPage').then((m) => ({ default: m.FilesPage })));
const ClassifyPage = lazy(() =>
  import('./pages/ClassifyPage').then((m) => ({ default: m.ClassifyPage })),
);
const ChatPage = lazy(() => import('./pages/ChatPage').then((m) => ({ default: m.ChatPage })));
const RulesPage = lazy(() => import('./pages/RulesPage').then((m) => ({ default: m.RulesPage })));
const SettingsPage = lazy(() =>
  import('./pages/SettingsPage').then((m) => ({ default: m.SettingsPage })),
);

/** 页面 chunk 加载占位：与 app-loading 同款 spinner（本地毫秒级，通常不可见）。 */
function PageFallback() {
  return (
    <div className="app-loading" role="status" aria-label="页面加载中">
      <div className="app-loading__spinner" />
    </div>
  );
}

export function App() {
  const isLoading = useSettingsStore((s) => s.isLoading);
  const initFailed = useSettingsStore((s) => s.initFailed);
  const initError = useSettingsStore((s) => s.error);
  const retryInit = useSettingsStore((s) => s.retryInit);
  const onboardingCompleted = useSettingsStore((s) => s.onboardingCompleted);

  // 配置加载中：显示加载态（避免引导流闪烁）
  if (isLoading) {
    return (
      <div className="app-loading" role="status" aria-label="应用加载中">
        <div className="app-loading__spinner" />
        <p>正在加载配置...</p>
      </div>
    );
  }

  // FE-M11：启动配置加载失败——显示错误卡片 + 重试，不再永久白屏。
  // 注意不能落回 OnboardingWizard：加载失败 ≠ 未完成引导，
  // 误入引导流会用初始值覆盖用户已有配置。
  if (initFailed) {
    return (
      <div className="app-loading" role="alert" aria-label="配置加载失败">
        <p>配置加载失败：{initError ?? '未知错误'}</p>
        <button
          type="button"
          className="btn btn--primary"
          data-testid="app-retry-init"
          onClick={() => void retryInit()}
        >
          重试
        </button>
      </div>
    );
  }

  // 首次启动未完成引导：显示引导向导（设计稿 §3：引导不可跳过）
  if (!onboardingCompleted) {
    return (
      <>
        <OnboardingWizard />
        <ToastContainer />
      </>
    );
  }

  // 已完成引导：显示主界面
  // HashRouter：Tauri 生产走自定义协议，无服务端路由回退，hash 路由两种环境都可用
  return (
    <HashRouter>
      <AppLayout>
        <Suspense fallback={<PageFallback />}>
          <Routes>
            <Route path="/" element={<FilesPage />} />
            <Route path="/classify" element={<ClassifyPage />} />
            <Route path="/chat" element={<ChatPage />} />
            <Route path="/rules" element={<RulesPage />} />
            <Route path="/settings" element={<SettingsPage />} />
          </Routes>
        </Suspense>
      </AppLayout>
      <ToastContainer />
    </HashRouter>
  );
}
