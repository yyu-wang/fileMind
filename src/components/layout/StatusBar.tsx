// 底部状态栏：订阅 settingsStore + fileStore 显示真实状态。
//
// 显示项（设计稿 05_交互原型 §状态栏）：
// - 推理模式标签（mode-badge 三色：紫=本地/蓝=云端/琥珀=混合）
// - 模型名
// - 文件数 + 索引状态
// - 选中数量 + 清除按钮（有选中时显示）
// - 版本号（右侧）

import { useFileStore } from '../../stores/fileStore';
import { useSettingsStore } from '../../stores/settingsStore';
import { MODE_COLORS, resolveDisplayModel } from '../../types/models';

const MODE_BADGE_CLASS: Record<string, string> = {
  Local: 'local',
  Cloud: 'cloud',
};

export function StatusBar() {
  const inferenceMode = useSettingsStore((s) => s.inferenceMode);
  const llmModel = useSettingsStore((s) => s.llmModel);
  const cloudModel = useSettingsStore((s) => s.cloudModel);
  const cloudConsentProvider = useSettingsStore((s) => s.cloudConsentProvider);
  const isLoadingSettings = useSettingsStore((s) => s.isLoading);
  const stats = useFileStore((s) => s.stats);
  const selectedIds = useFileStore((s) => s.selectedIds);
  const clearSelection = useFileStore((s) => s.clearSelection);

  const modeColor = MODE_COLORS[inferenceMode];
  const badgeClass = MODE_BADGE_CLASS[inferenceMode] ?? 'local';
  const displayModel = resolveDisplayModel(
    inferenceMode,
    llmModel,
    cloudModel,
    cloudConsentProvider,
  );

  return (
    <footer className="statusbar" aria-label="状态栏">
      <span className={`status-item mode-badge ${badgeClass}`} title="当前推理模式">
        <span className="mode-dot" style={{ background: modeColor.dot }} aria-hidden />
        {modeColor.label}
      </span>
      <span className="status-divider" aria-hidden />
      <span className="status-item" title="当前模型">
        {displayModel}
      </span>
      <span className="status-divider" aria-hidden />
      <span className="status-item" title="文件数与索引状态">
        {isLoadingSettings ? '加载中...' : stats ? `${stats.total_files} 文件` : '0 文件'}
      </span>
      {selectedIds.length > 0 && (
        <>
          <span className="status-divider" aria-hidden />
          <span className="status-item" title="已选中文件数">
            选中 {selectedIds.length}
          </span>
          <button
            type="button"
            className="status-bar__link"
            onClick={clearSelection}
            title="清除所有选中"
            aria-label="清除选中"
          >
            清除
          </button>
        </>
      )}
      <div className="status-right">
        <span className="status-item" title="版本号">
          v1.0.0
        </span>
      </div>
    </footer>
  );
}
