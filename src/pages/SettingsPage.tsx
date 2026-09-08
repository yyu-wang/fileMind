// 设置页（对齐交互原型 §设置）：main-header + main-content > settings-layout(720 居中)。
//
// 文档结构：
//   <div class="page settings-page">
//     <header class="main-header"><h1>设置</h1></header>
//     <div class="main-content">
//       <div class="settings-layout">  // max-width 720px 居中
//         <InferenceModeSection/>      // 推理模式 + mode-selector(local/cloud)
//         <CloudApiKeySection/>         // AI 模型配置 + setting-row
//         <OllamaStatusCard/>           // Ollama 环境状态
//         <EmbeddingModelSection/>      // Embedding 模型 + model-item
//         <ThemeSection/>               // 主题
//         <AboutSection/>               // 关于 + setting-row
//       </div>
//     </div>
//   </div>
//
// 挂载时触发一次 Ollama 探测（probeOllama），区块各自读取 store 结果。

import { useEffect } from 'react';
import { AboutSection } from '../components/settings/AboutSection';
import { CloudApiKeySection } from '../components/settings/CloudApiKeySection';
import { CloudProviderManager } from '../components/settings/CloudProviderManager';
import { EmbeddingModelSection } from '../components/settings/EmbeddingModelSection';
import { InferenceModeSection } from '../components/settings/InferenceModeSection';
import { OllamaStatusCard } from '../components/settings/OllamaStatusCard';
import { ThemeSection } from '../components/settings/ThemeSection';
import { useSettingsStore } from '../stores/settingsStore';

export function SettingsPage() {
  const probeOllama = useSettingsStore((s) => s.probeOllama);

  useEffect(() => {
    void probeOllama();
  }, [probeOllama]);

  return (
    <div className="page settings-page">
      <header className="main-header">
        <h1>设置</h1>
        <span className="subtitle">管理推理模式、生成模型与本地 Ollama 环境</span>
      </header>
      <div className="main-content">
        <div className="settings-layout">
          <InferenceModeSection />
          <CloudProviderManager />
          <CloudApiKeySection />
          <OllamaStatusCard />
          <EmbeddingModelSection />
          <ThemeSection />
          <AboutSection />
        </div>
      </div>
    </div>
  );
}
