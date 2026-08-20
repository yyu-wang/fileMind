// 设置页：推理模式 / Ollama 环境 / Embedding 模型 三个区块（T6.7）。
//
// 挂载时触发一次 Ollama 探测（probeOllama），区块各自读取 store 结果；
// OllamaStatusCard 提供手动「重新检测」。

import { useEffect } from 'react';
import { EmbeddingModelSection } from '../components/settings/EmbeddingModelSection';
import { InferenceModeSection } from '../components/settings/InferenceModeSection';
import { OllamaStatusCard } from '../components/settings/OllamaStatusCard';
import { useSettingsStore } from '../stores/settingsStore';

export function SettingsPage() {
  const probeOllama = useSettingsStore((s) => s.probeOllama);

  useEffect(() => {
    void probeOllama();
  }, [probeOllama]);

  return (
    <div className="settings-page">
      <header className="settings-page__header">
        <h2 className="settings-page__title">设置</h2>
        <p className="settings-page__desc">管理推理模式、生成模型与本地 Ollama 环境。</p>
      </header>
      <div className="settings-page__body">
        <InferenceModeSection />
        <OllamaStatusCard />
        <EmbeddingModelSection />
      </div>
    </div>
  );
}
