// 「启用规则」开关行（对齐交互原型 §规则编辑 的 setting-row + toggle）。

interface RuleEnabledToggleProps {
  /** 当前是否启用 */
  enabled: boolean;
  /** 切换启用状态 */
  onToggle: () => void;
}

export function RuleEnabledToggle({ enabled, onToggle }: RuleEnabledToggleProps) {
  return (
    <div className="setting-row" style={{ border: 'none', padding: 0 }}>
      <div className="setting-label">
        <div className="name">启用规则</div>
        <div className="desc">禁用后该规则不参与分类匹配</div>
      </div>
      <div className="setting-control">
        <button
          type="button"
          className={`toggle${enabled ? ' on' : ''}`}
          aria-label={enabled ? '禁用规则' : '启用规则'}
          aria-pressed={enabled}
          data-testid="rule-toggle"
          onClick={onToggle}
        />
      </div>
    </div>
  );
}
