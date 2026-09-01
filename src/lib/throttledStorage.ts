// 聊天 persist 的节流写盘层。
//
// zustand persist 在每次 set 后都会同步 setItem；流式聊天每个 token 触发一次
// set，若直接写 localStorage 会把「整个对话历史的 JSON.stringify」放大到
// token 级频率。此处将 setItem 合并到 1s 窗口内至多一次；流终态
// （finishStream/clearHistory）与页面隐藏/关闭时 flush，正常退出不丢数据，
// 异常崩溃至多丢最后一秒的持久化副本。

import type { StateStorage } from 'zustand/middleware';

/** 合并写盘窗口（ms）。 */
const THROTTLE_MS = 1000;

const pending = new Map<string, string>();
let timer: ReturnType<typeof setTimeout> | null = null;

function flush(): void {
  if (timer) {
    clearTimeout(timer);
    timer = null;
  }
  for (const [name, value] of pending) {
    localStorage.setItem(name, value);
  }
  pending.clear();
}

export const throttledLocalStorage: StateStorage = {
  getItem: (name) => localStorage.getItem(name),
  setItem: (name, value) => {
    pending.set(name, value);
    if (timer) return;
    timer = setTimeout(flush, THROTTLE_MS);
  },
  removeItem: (name) => {
    pending.delete(name);
    localStorage.removeItem(name);
  },
};

/** 立即写盘 pending 数据（流终态 / 页面隐藏时调用）。 */
export function flushThrottledStorage(): void {
  flush();
}

if (typeof document !== 'undefined') {
  document.addEventListener('visibilitychange', () => {
    if (document.visibilityState === 'hidden') flush();
  });
  window.addEventListener('beforeunload', flush);
}
