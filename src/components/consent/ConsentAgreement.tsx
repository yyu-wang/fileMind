// 共享云端知情同意书滚动内容（T7.5 滚动到底门控）。
//
// 安全约束（07-§3）：切换云端前必须显示同意书并满足「滚动到底 + 勾选」
// 双重条件才能确认。本组件承载同意书全文与滚动到底检测，父组件持
// bottomReached state 联动 checkbox / 确认按钮；内容无需滚动即可全部
// 可见时（无滚动条）视为已读到底，直接放行。
//
// 文案依据 07-§3 数据最小化：明确列出「发送内容摘要 / 文件名脱敏 /
// 路径脱敏 / RAG 片段本地拼接」等实际发送范围，而非泛泛的"你的文件内容"。

import { useEffect, useRef } from 'react';

interface ConsentAgreementProps {
  /** 同意书版本号（与后端 signCloudConsent 传入一致）。 */
  version: string;
  /** 滚动到底状态变化回调（true = 已读完全部内容）。 */
  onBottomReached: (reached: boolean) => void;
}

/** 滚动到底判定容差（px）：允许末行视觉残差。 */
const SCROLL_BOTTOM_TOLERANCE_PX = 4;

export function ConsentAgreement({ version, onBottomReached }: ConsentAgreementProps) {
  const scrollRef = useRef<HTMLDivElement | null>(null);
  // 用 ref 持最新回调：effect 与 onScroll 均经 ref 调用，避免闭包过期，
  // 也规避 exhaustive-deps 对 onScroll/挂载检测的告警。
  const onBottomReachedRef = useRef(onBottomReached);

  useEffect(() => {
    onBottomReachedRef.current = onBottomReached;
  });

  useEffect(() => {
    // 首次挂载检测：内容全部可见（无需滚动）即视为已读到底；
    // 仅挂载时检测一次（依赖仅 ref，无闭包过期问题），用户滚动由 onScroll 持续上报
    const el = scrollRef.current;
    if (el) {
      const reached =
        el.scrollHeight - el.scrollTop - el.clientHeight <= SCROLL_BOTTOM_TOLERANCE_PX;
      onBottomReachedRef.current(reached);
    }
  }, []);

  const handleScroll = () => {
    const el = scrollRef.current;
    if (!el) return;
    const reached = el.scrollHeight - el.scrollTop - el.clientHeight <= SCROLL_BOTTOM_TOLERANCE_PX;
    onBottomReachedRef.current(reached);
  };

  return (
    <div
      className="consent-agreement__scroll"
      ref={scrollRef}
      onScroll={handleScroll}
      tabIndex={0}
      role="document"
      aria-label="知情同意书内容"
    >
      <div className="onboarding__hint onboarding__hint--warn">
        <div className="onboarding__hint-title">⚠️ 请仔细阅读以下内容</div>
        <div className="onboarding__hint-body">
          <p>
            <strong>选择云端模式意味着以下数据将发送到第三方 AI 服务提供商</strong>
            （如 OpenAI、DeepSeek）并在其服务器上处理以生成回答：
          </p>
          <ul>
            <li>
              <strong>内容摘要</strong>：仅发送文件内容的前 500 字符摘要，不发送完整文件内容
              （可在设置中调整）
            </li>
            <li>
              <strong>文件名脱敏</strong>：真实文件名替换为编号（如 file_001），不发送原名
            </li>
            <li>
              <strong>路径脱敏</strong>：仅发送相对路径，不发送完整文件路径
            </li>
            <li>
              <strong>RAG 问答片段</strong>：检索到的文档片段在本地拼接后发送给云端，片段需经你确认
            </li>
          </ul>
          <p>
            <strong>风险提示</strong>：虽然提供商有保密政策，但数据一旦离开你的设备，无法保证
            不被云端存储或披露。
          </p>
          <p>
            <strong>你可以随时撤回同意</strong>：在设置中点击「撤回并切回本地」后，应用自动
            切换回本地模式，不会再上传任何数据。
          </p>
          <p className="consent-agreement__version-line">同意书版本：{version}</p>
        </div>
      </div>
    </div>
  );
}
