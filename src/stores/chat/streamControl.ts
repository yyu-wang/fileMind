// 聊天流控制：seq 去重与看门狗（模块级瞬态，不进 store 持久化）。
//
// 与 store 分离的原因：这些是「非响应式」的模块级状态与计时器，被发送、事件分发、
// 收尾、清空等多个动作共享；抽成独立模块后各 action 工厂按需引用，避免反向依赖 store。
//
// FE-C2/FE-C3 语义：
// - activeSeq：当前流的 seq（null = 无流 / 新流 invoke 未返回）
// - lastSeq：最近一次 invoke 返回的 seq（收编「首帧先于 invoke 返回」的事件）
// - 看门狗 timer：空闲 / 总量超时兜底复位 isStreaming

/** FE-C3：流空闲看门狗——上一个事件后超过该时长无新事件视为流挂起。 */
const STREAM_IDLE_TIMEOUT_MS = 90_000;
/** FE-C3：流整体时长上限（含正常长回答的余量）。 */
const STREAM_TOTAL_TIMEOUT_MS = 600_000;

let activeSeq: number | null = null;
let lastSeq = 0;
let idleTimer: ReturnType<typeof setTimeout> | null = null;
let totalTimer: ReturnType<typeof setTimeout> | null = null;

/** FE-C2：作废当前流（旧流残余事件此后全部失效）。 */
export function resetActiveSeq(): void {
  activeSeq = null;
}

/** FE-C2：记录新流 seq，此后只处理该 seq 的事件。 */
export function setActiveSeq(seq: number): void {
  lastSeq = seq;
  activeSeq = seq;
}

/**
 * FE-C2：按 seq 判定事件是否属于当前流。
 *
 * 只处理当前流的事件，旧流残余（含迟到 error）一律丢弃，避免串入新回答或
 * 误杀进行中的新流；新流 invoke 未返回时（activeSeq 为空）仅收编「seq 比
 * lastSeq 大」的首批事件（旧流残余 seq 等于旧值，不会被收编）。
 *
 * Args:
 *   requestSeq: 事件携带的流 seq
 *   isStreaming: 当前是否处于流式状态
 *
 * Returns:
 *   该事件是否应继续处理
 */
export function acceptEvent(requestSeq: number, isStreaming: boolean): boolean {
  if (activeSeq === null) {
    if (isStreaming && requestSeq > lastSeq) {
      lastSeq = requestSeq;
      activeSeq = requestSeq;
    } else {
      return false;
    }
  } else if (requestSeq !== activeSeq) {
    return false;
  }
  return true;
}

/** FE-C3：清看门狗（终态/清空/发送失败时调用）。 */
export function stopWatchdog(): void {
  if (idleTimer) clearTimeout(idleTimer);
  if (totalTimer) clearTimeout(totalTimer);
  idleTimer = null;
  totalTimer = null;
}

/**
 * FE-C3：（重新）启动看门狗。每个已处理事件都会重置空闲计时；
 * 任一超时触发时若仍在流式 → 置错误并复位全部流式状态。
 */
export function restartWatchdog(onTimeout: () => void): void {
  if (idleTimer) clearTimeout(idleTimer);
  idleTimer = setTimeout(onTimeout, STREAM_IDLE_TIMEOUT_MS);
  if (!totalTimer) {
    totalTimer = setTimeout(onTimeout, STREAM_TOTAL_TIMEOUT_MS);
  }
}
