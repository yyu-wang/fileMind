// 智能分类页初始态：说明 + 开始分类 + 进入分类历史。
//
// 纯展示组件：开始按钮的文案与禁用态由页面从 resolveClassifyPageView 取好后传入
// （「选中 / 未整理目标 / 是否已扫描」的判定都在 lib/classifyView.ts）。

import type { StartButtonState } from '@/lib/classifyView';

interface ClassifyIntroPanelProps {
  /** 开始分类按钮的文案与禁用态 */
  startButton: StartButtonState;
  /** 开始分类（生成预览） */
  onStart: () => void;
  /** 进入分类历史视图 */
  onOpenHistory: () => void;
}

export function ClassifyIntroPanel({
  startButton,
  onStart,
  onOpenHistory,
}: ClassifyIntroPanelProps) {
  return (
    <div className="classify-page__intro">
      <p className="classify-page__intro-title">按规则与文件类型自动整理</p>
      <p className="classify-page__intro-sub">
        预览分类结果后执行；规则与类型识别均未命中的文件会进入「待确认」列表。
      </p>
      <button
        type="button"
        className="btn btn--primary"
        data-testid="classify-start"
        onClick={onStart}
        disabled={startButton.disabled}
      >
        {startButton.label}
      </button>
      <button
        type="button"
        className="btn btn--ghost classify-page__history-btn"
        onClick={onOpenHistory}
      >
        查看分类历史
      </button>
    </div>
  );
}
