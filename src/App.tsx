import { Routes, Route } from 'react-router-dom';
import { FilesPage } from './pages/FilesPage';
import { ClassifyPage } from './pages/ClassifyPage';
import { ChatPage } from './pages/ChatPage';
import { RulesPage } from './pages/RulesPage';
import { SettingsPage } from './pages/SettingsPage';

export function App() {
  return (
    <Routes>
      <Route path="/" element={<FilesPage />} />
      <Route path="/classify" element={<ClassifyPage />} />
      <Route path="/chat" element={<ChatPage />} />
      <Route path="/rules" element={<RulesPage />} />
      <Route path="/settings" element={<SettingsPage />} />
    </Routes>
  );
}
