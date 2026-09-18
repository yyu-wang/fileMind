// 知识问答页底部：引擎状态提示 + 输入框。
//
// 纯展示组件：引擎状态由页面注入；输入框的禁用态是「引擎未就绪」与
// 「正在流式输出」的组合结果，组合方式固定在这里，页面不再重复判断。

import { ChatInput } from './ChatInput';

interface ChatInputAreaProps {
  /** AI 引擎是否就绪（未就绪时禁用输入并给出提示） */
  engineReady: boolean;
  /** 引擎是否启动失败（提示改为失败文案 + 重试入口） */
  engineFailed: boolean;
  /** 是否正在流式输出（输出期间不允许再次发送） */
  streaming: boolean;
  /** 重试启动 AI 引擎 */
  onRetryEngine: () => void;
  /** 发送问题 */
  onSend: (content: string) => void;
}

export function ChatInputArea({
  engineReady,
  engineFailed,
  streaming,
  onRetryEngine,
  onSend,
}: ChatInputAreaProps) {
  return (
    <div className="chat-input-area">
      {!engineReady && (
        <div className="chat-page__engine-hint" role="status">
          {engineFailed ? (
            <>
              <span>AI 引擎启动失败，暂时无法提问。</span>
              <button type="button" className="status-bar__link" onClick={onRetryEngine}>
                重试
              </button>
            </>
          ) : (
            <span>AI 引擎启动中，就绪后可开始问答…</span>
          )}
        </div>
      )}
      <ChatInput disabled={streaming || !engineReady} onSend={onSend} />
    </div>
  );
}
