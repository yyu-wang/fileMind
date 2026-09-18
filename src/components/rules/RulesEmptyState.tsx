// 规则页空态：尚无任何自定义规则时的占位与新建入口。

import { EmptyState } from '@/components/common/StateViews';

interface RulesEmptyStateProps {
  /** 打开新建规则表单 */
  onNew: () => void;
}

export function RulesEmptyState({ onNew }: RulesEmptyStateProps) {
  return (
    <EmptyState
      title="暂无自定义规则"
      description="内置类型识别仍会自动分类；新建规则可覆盖默认行为。"
      action={
        <button type="button" className="btn btn--primary" onClick={onNew}>
          + 新建规则
        </button>
      }
    />
  );
}
