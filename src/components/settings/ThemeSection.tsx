// 外观区块（设置页）：三态主题切换（跟随系统 / 亮色 / 暗色）。
//
// 切换直接调 settingsStore.setTheme，持久化并应用到 <html data-theme>。

import { useSettingsStore } from '../../stores/settingsStore';
import { ThemeMode } from '../../types/models';

const THEME_OPTIONS: Array<{ value: ThemeMode; icon: string; label: string }> = [
  { value: ThemeMode.System, icon: '🖥️', label: '跟随系统' },
  { value: ThemeMode.Light, icon: '☀️', label: '亮色' },
  { value: ThemeMode.Dark, icon: '🌙', label: '暗色' },
];

export function ThemeSection() {
  const theme = useSettingsStore((s) => s.theme);
  const setTheme = useSettingsStore((s) => s.setTheme);

  return (
    <section className="settings-section" aria-labelledby="settings-theme-title">
      <h3 id="settings-theme-title" className="settings-section__title">
        外观
      </h3>
      <p className="settings-section__desc">选择亮色或暗色主题，或跟随系统自动切换。</p>
      <div className="theme-segmented" role="radiogroup" aria-label="主题模式">
        {THEME_OPTIONS.map((opt) => (
          <button
            key={opt.value}
            type="button"
            className={`theme-segmented__item ${theme === opt.value ? 'selected' : ''}`}
            onClick={() => setTheme(opt.value)}
            aria-pressed={theme === opt.value}
          >
            <span className="theme-segmented__icon" aria-hidden>
              {opt.icon}
            </span>
            {opt.label}
          </button>
        ))}
      </div>
    </section>
  );
}
