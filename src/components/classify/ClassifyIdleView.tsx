// 分类页初始态区域：介绍面板（可开始分类）与分类历史视图，二者互斥。

import type { StartButtonState } from '@/lib/classifyView';

import { ClassifyHistoryView } from './ClassifyHistoryView';
import { ClassifyIntroPanel } from './ClassifyIntroPanel';

interface ClassifyIdleViewProps {
  /** 开始分类按钮的文案与禁用态 */
  startButton: StartButtonState;
  /** 是否展示分类历史视图（否则展示介绍面板） */
  historyOpen: boolean;
  /** 开始分类（生成预览） */
  onStart: () => void;
  /** 进入分类历史视图 */
  onOpenHistory: () => void;
  /** 退出分类历史视图 */
  onCloseHistory: () => void;
}

export function ClassifyIdleView({
  startButton,
  historyOpen,
  onStart,
  onOpenHistory,
  onCloseHistory,
}: ClassifyIdleViewProps) {
  if (historyOpen) {
    return <ClassifyHistoryView onBack={onCloseHistory} />;
  }
  return (
    <ClassifyIntroPanel startButton={startButton} onStart={onStart} onOpenHistory={onOpenHistory} />
  );
}
