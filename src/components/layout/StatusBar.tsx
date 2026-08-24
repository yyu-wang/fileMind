// 底部状态栏：订阅 settingsStore + fileStore 显示真实状态。
//
// 显示项（设计稿 §2 状态栏）：
// - 推理模式标签（三色：紫=本地/蓝=云端/琥珀=混合，当前仅 Local/Cloud）
// - 模型名
// - 文件数 + 索引状态
// - 版本号

import { useFileStore } from '../../stores/fileStore';
import { useSettingsStore } from '../../stores/settingsStore';
import { MODE_COLORS } from '../../types/models';

export function StatusBar() {
  const inferenceMode = useSettingsStore((s) => s.inferenceMode);
  const llmModel = useSettingsStore((s) => s.llmModel);
  const isLoadingSettings = useSettingsStore((s) => s.isLoading);
  const stats = useFileStore((s) => s.stats);

  const modeColor = MODE_COLORS[inferenceMode];

  return (
    <footer className="status-bar" aria-label="状态栏">
      <span
        className="status-bar__tag"
        style={{ background: modeColor.bg, color: modeColor.dot }}
        title="当前推理模式"
      >
        ● {modeColor.label}
      </span>
      <span className="status-bar__sep" aria-hidden>
        |
      </span>
      <span className="status-bar__text" title="当前模型">
        {llmModel}
      </span>
      <span className="status-bar__sep" aria-hidden>
        |
      </span>
      <span className="status-bar__text" title="文件数与索引状态">
        {isLoadingSettings
          ? '加载中...'
          : stats
            ? `${stats.total_files} 文件 · 索引就绪`
            : '0 文件'}
      </span>
      <span className="status-bar__spacer" />
      <span className="status-bar__text status-bar__text--muted" title="版本号">
        v0.1.0
      </span>
    </footer>
  );
}
