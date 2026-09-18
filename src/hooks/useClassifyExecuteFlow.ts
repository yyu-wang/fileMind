// 「分类方式」弹窗流程（从 ClassifyPage 的 pendingExecute 状态与 handleModeChosen 抽出）。
//
// 头部点「仅执行无冲突项 / 确认执行全部」只记录待执行参数并弹出选择框，
// 选完 move/copy 才真正调 store 执行。

import { useState } from 'react';

import { useClassifyStore, type ClassifyExecMode } from '@/stores/classifyStore';

/** `useClassifyExecuteFlow` 的对外出口。 */
export interface ClassifyExecuteFlow {
  /** 待执行的冲突策略（null 表示弹窗关闭） */
  pendingExecute: { resolveConflicts: boolean } | null;
  /** 请求执行：打开「分类方式」弹窗 */
  requestExecute: (resolveConflicts: boolean) => void;
  /** 关闭弹窗（不执行） */
  cancelExecute: () => void;
  /** 选定分类方式后真正执行 */
  chooseMode: (mode: ClassifyExecMode) => void;
}

/** 管理「分类方式」选择弹窗的待执行参数与执行派发。 */
export function useClassifyExecuteFlow(): ClassifyExecuteFlow {
  const execute = useClassifyStore((s) => s.execute);
  const [pendingExecute, setPendingExecute] = useState<{ resolveConflicts: boolean } | null>(null);

  const chooseMode = (mode: ClassifyExecMode) => {
    if (!pendingExecute) return;
    const { resolveConflicts } = pendingExecute;
    setPendingExecute(null);
    void execute(resolveConflicts, mode);
  };

  return {
    pendingExecute,
    requestExecute: (resolveConflicts: boolean) => setPendingExecute({ resolveConflicts }),
    cancelExecute: () => setPendingExecute(null),
    chooseMode,
  };
}
