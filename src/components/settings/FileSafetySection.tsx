// 文件操作安全网区块（设置页）：Dry-run / 操作日志+撤销 / 删除走回收站 / 日志保留天数。
//
// 原型 05_交互原型 §设置页文件操作安全网区。AppConfig 后端无对应字段
// （dry_run / log_retention_days / trash_enabled），P1 不支持持久化，
// 留 disabled 占位保持原型视觉一致；后端改造属 T11+ 后续任务。
// 注：Dry-run 预览模式当前已在分类页 ClassifyModeDialog 实现为页级开关，
// 此处全局开关需后端支持，暂占位。

interface ToggleRowProps {
  name: string;
  desc: string;
  defaultOn?: boolean;
}

/** 单行 toggle 占位：disabled checkbox + 文字，模拟原型开关视觉效果。 */
function ToggleRow({ name, desc, defaultOn = true }: ToggleRowProps) {
  return (
    <div className="settings-row">
      <div className="settings-field">
        <label className="settings-field__label">{name}</label>
        <span className="settings-field__hint">{desc}</span>
      </div>
      <input
        type="checkbox"
        className="settings-toggle-placeholder"
        defaultChecked={defaultOn}
        disabled
        aria-label={`${name}（P1 不支持）`}
      />
    </div>
  );
}

export function FileSafetySection() {
  return (
    <section className="settings-section" aria-labelledby="settings-safety-title">
      <h3 id="settings-safety-title" className="settings-section__title">
        📁 文件操作安全网
      </h3>
      <p className="settings-section__desc">
        保护你的文件不被误操作。以下开关需后端支持，P1 暂未启用。
      </p>
      <ToggleRow
        name="Dry-run 预览模式"
        desc="执行前必须先预览分类方案（当前已在分类页支持，全局开关待后端）"
      />
      <ToggleRow name="操作日志 + 撤销" desc="保留操作历史，支持一键撤销" />
      <ToggleRow name="删除走系统回收站" desc="删除的文件进入回收站而非永久删除" />
      <div className="settings-row">
        <div className="settings-field">
          <label htmlFor="log-retention-select" className="settings-field__label">
            日志保留天数
          </label>
          <span className="settings-field__hint">操作历史自动清理周期</span>
        </div>
        <select id="log-retention-select" className="settings-select" disabled defaultValue="30">
          <option value="30">30 天</option>
          <option value="90">90 天</option>
          <option value="0">永久保留</option>
        </select>
      </div>
    </section>
  );
}
