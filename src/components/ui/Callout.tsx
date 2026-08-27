// Callout 通用提示框组件（对齐交互原型 §Callout）。
//
// 用于页面/面板中的信息提示，支持四种语义色：
//   warn    — 警告（如隐私同意书）
//   success — 成功（如 Ollama 检测通过）
//   info    — 信息（如检测中）
//   amber   — 注意（如混合模式提示）
//
// 用法：
//   <Callout variant="warn" title="注意">
//     <p>提示内容</p>
//   </Callout>

import type { ReactNode } from 'react';

type CalloutVariant = 'warn' | 'success' | 'info' | 'amber';

interface CalloutProps {
  /** 语义色：warn/success/info/amber */
  variant: CalloutVariant;
  /** 可选标题（加粗显示在顶部） */
  title?: string;
  /** 内容 */
  children: ReactNode;
}

export function Callout({ variant, title, children }: CalloutProps) {
  return (
    <div className={`callout ${variant}`}>
      {title && <div className="callout-title">{title}</div>}
      {children}
    </div>
  );
}
