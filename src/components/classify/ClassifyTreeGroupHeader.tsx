// 分类预览树的分组面板组头：折叠箭头 + 组名 +（待确认组的）全选 + 文件计数。
//
// 折叠交互集中在组头：整行点击切换折叠态，全选框需 stopPropagation 以免连带折叠。

interface ClassifyTreeGroupHeaderProps {
  /** 组展示名（分类名 / 「⚠️ 冲突」/「❓ 待确认」） */
  name: string;
  /** 组内文件数（计数文案） */
  count: number;
  /** 是否为冲突组（计数文案追加「需处理」） */
  conflict: boolean;
  /** 折叠态与该组头的折叠动作 */
  collapse: { collapsed: boolean; onToggle: () => void };
  /** 待确认组的全选框（其余组不渲染） */
  selectAll: { visible: boolean; checked: boolean; onToggle: () => void };
}

export function ClassifyTreeGroupHeader({
  name,
  count,
  conflict,
  collapse,
  selectAll,
}: ClassifyTreeGroupHeaderProps) {
  return (
    <div
      className="tree-node parent"
      onClick={collapse.onToggle}
      role="button"
      aria-expanded={!collapse.collapsed}
    >
      <span className={`arrow${collapse.collapsed ? '' : ' expanded'}`} aria-hidden />
      <span>{name}</span>
      {selectAll.visible && (
        <label className="classify-preview__select-all" onClick={(e) => e.stopPropagation()}>
          <input
            type="checkbox"
            checked={selectAll.checked}
            onChange={selectAll.onToggle}
            aria-label="全选待确认文件"
          />
        </label>
      )}
      <span className="node-count">
        {count} 文件{conflict ? ' · 需处理' : ''}
      </span>
    </div>
  );
}
