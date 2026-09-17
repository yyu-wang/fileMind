// 执行令牌（FE-C5）：取号、作废与持有判定。
//
// 从 runActions 拆出（该文件逼近 .ts 警告阈值 150）。计数器与它的**全部写入方**
// （作废 / 取号）必须在同一模块，故三者一起落在这里。
//
// 语义：每次 execute 递增取号，reset() 通过 invalidateExecution() 再递增作废在途
// 执行；在途执行在每个 await 恢复点用 holdsToken 校验，失配即放弃写状态——
// 防止「用户已 reset 重来」后被旧循环的收尾 set 覆盖回 Cancelled/Done。

import { ClassifyStatus } from '@/types/models';
import type { ExecControl } from './executor';
import type { ClassifyState } from './types';

let execToken = 0;

/** 作废在途执行（reset 调用；循环恢复后因令牌失配静默退出）。 */
export function invalidateExecution(): void {
  execToken += 1;
}

/** 取执行令牌号并复位控制开关，同时把状态切到 Running。 */
export function beginExecution(
  set: (patch: Partial<ClassifyState>) => void,
  control: ExecControl,
  total: number,
): number {
  const token = ++execToken;
  control.paused = false;
  control.cancelled = false;
  set({ status: ClassifyStatus.Running, error: null, progress: { done: 0, total } });
  return token;
}

/** 当前执行是否仍持有令牌（reset 作废在途执行后为 false）。 */
export function holdsToken(token: number): boolean {
  return token === execToken;
}
