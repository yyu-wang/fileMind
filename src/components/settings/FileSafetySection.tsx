// 文件操作安全网区块（设置页）：Dry-run / 操作日志+撤销 / 删除走回收站 / 日志保留天数。
//
// 原型 05_交互原型 §设置页文件操作安全网区。AppConfig 后端无对应字段
// （dry_run / log_retention_days / trash_enabled），P1 不支持持久化，
// 留 disabled 占位保持原型视觉一致；后端改造属 T11+ 后续任务。
// 注：Dry-run 预览模式当前已在分类页 ClassifyModeDialog 实现为页级开关，
// 此处全局开关需后端支持，暂占位。

import { useState } from 'react';

interface ToggleRowProps {
  name: string;
  desc: string;
  defaultOn?: boolean;
}

/** 单行 toggle 占位：原型 .toggle 视觉（disabled 状态不可切换）。 */
function ToggleRow({ name, desc, defaultOn = true }: ToggleRowProps) {
  const [on, setOn] = useState(defaultOn);
  return (
    <div className="setting-row">
      <div className="setting-label">
        <div className="name">{name}</div>
        <div className="desc">{desc}</div>
      </div>
      <div className="setting-control">
        <button
          type="button"
          className={`toggle${on ? ' on' : ''}`}
          aria-label={`${name}（P1 不支持）`}
          aria-pressed={on}
          disabled
          onClick={() => setOn((v) => !v)}
        />
      </div>
    </div>
  );
}

export function FileSafetySection() {
  return (
    <section className="settings-section" aria-labelledby="settings-safety-title">
      <h3 id="settings-safety-title" className="settings-section__title">
        📁 文件操作安全网
      </h3>
      <p className="section-desc">保护你的文件不被误操作。以下开关需后端支持，P1 暂未启用。</p>
      <ToggleRow
        name="Dry-run 预览模式"
        desc="执行前必须先预览分类方案（当前已在分类页支持，全局开关待后端）"
      />
      <ToggleRow name="操作日志 + 撤销" desc="保留操作历史，支持一键撤销" />
      <ToggleRow name="删除走系统回收站" desc="删除的文件进入回收站而非永久删除" />
      <div className="setting-row">
        <div className="setting-label">
          <div className="name">日志保留天数</div>
          <div className="desc">操作历史自动清理周期</div>
        </div>
        <div className="setting-control">
          <select
            className="input"
            disabled
            defaultValue="30"
            aria-label="日志保留天数（P1 不支持）"
          >
            <option value="30">30 天</option>
            <option value="90">90 天</option>
            <option value="0">永久保留</option>
          </select>
        </div>
      </div>
    </section>
  );
}
