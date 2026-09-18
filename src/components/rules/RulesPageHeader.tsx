// 规则编辑页头部：标题 + 副标题 + 「新建规则」按钮。

interface RulesPageHeaderProps {
  /** 打开新建规则表单 */
  onNew: () => void;
}

export function RulesPageHeader({ onNew }: RulesPageHeaderProps) {
  return (
    <header className="main-header">
      <h1>规则编辑</h1>
      <span className="subtitle">分类规则与分类体系管理</span>
      <div className="header-actions">
        <button
          type="button"
          className="btn btn--primary btn--sm"
          onClick={onNew}
          data-testid="rules-new"
        >
          <svg
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth={2}
            strokeLinecap="round"
            strokeLinejoin="round"
            style={{
              width: 13,
              height: 13,
              display: 'inline-block',
              verticalAlign: '-2px',
              marginRight: 4,
            }}
            aria-hidden
          >
            <path d="M12 5v14M5 12h14" />
          </svg>
          新建规则
        </button>
      </div>
    </header>
  );
}
