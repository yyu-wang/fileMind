import { Routes, Route } from 'react-router-dom';
import { AppLayout } from './components/layout/AppLayout';
import { OnboardingWizard } from './components/onboarding/OnboardingWizard';
import { FilesPage } from './pages/FilesPage';
import { ClassifyPage } from './pages/ClassifyPage';
import { ChatPage } from './pages/ChatPage';
import { RulesPage } from './pages/RulesPage';
import { SettingsPage } from './pages/SettingsPage';
import { useSettingsStore } from './stores/settingsStore';

export function App() {
  const isLoading = useSettingsStore((s) => s.isLoading);
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

  // 首次启动未完成引导：显示引导向导（设计稿 §3：引导不可跳过）
  if (!onboardingCompleted) {
    return <OnboardingWizard />;
  }

  // 已完成引导：显示主界面
  return (
    <AppLayout>
      <Routes>
        <Route path="/" element={<FilesPage />} />
        <Route path="/classify" element={<ClassifyPage />} />
        <Route path="/chat" element={<ChatPage />} />
        <Route path="/rules" element={<RulesPage />} />
        <Route path="/settings" element={<SettingsPage />} />
      </Routes>
    </AppLayout>
  );
}
