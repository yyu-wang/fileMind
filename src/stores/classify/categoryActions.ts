// 分类缓存动作：幂等加载与强制重拉分类列表（手动分类下拉的数据源）。
//
// 与 store 分离的原因：两者的失败契约不同（一个写 error 横幅、一个静默），
// 各自带着契约说明的注释留在 create 回调里会把回调推过函数行数阈值。

import { fileIpc } from '@/lib/ipc';
import { ipcErrorMessage } from '@/lib/ipcError';
import type { ClassifyState } from './types';

/** 分类缓存动作所需的最小依赖面。 */
export interface CategoryDeps {
  /** 写入状态（zustand 的 set） */
  set: (patch: Partial<ClassifyState>) => void;
  /** 读当前状态（zustand 的 get） */
  get: () => ClassifyState;
}

/**
 * 生成分类缓存动作（供 store 展开进 create 的返回对象）。
 *
 * Args:
 *   deps: store 注入的依赖面
 *
 * Returns:
 *   加载与强制重拉分类列表动作
 */
export function createCategoryActions(
  deps: CategoryDeps,
): Pick<ClassifyState, 'loadCategories' | 'refreshCategories'> {
  const { set, get } = deps;
  return {
    loadCategories: async () => {
      // 幂等：已有缓存不重复拉取（进入预览时调用一次即可）
      if (get().categories.length > 0) return;
      try {
        const result = await fileIpc.listCategories();
        if (result.status === 'ok') {
          set({ categories: result.data });
        } else {
          set({ error: result.error });
        }
      } catch (err) {
        // 调用方是 `void get().loadCategories()`，抛出去只会变成 unhandled rejection
        set({ error: ipcErrorMessage(err) });
      }
    },

    refreshCategories: async () => {
      // 绕过幂等缓存强制重拉。失败保持旧缓存静默返回：调用方（规则页增删分类）
      // 已有自己的成功/失败提示，此处再 set error 会把规则页操作误报为分类页错误。
      try {
        const result = await fileIpc.listCategories();
        if (result.status === 'ok') {
          set({ categories: result.data });
        }
      } catch (err) {
        // 同「静默返回」契约：只记日志，不污染分类页错误横幅
        console.warn('[classify] 刷新分类缓存失败:', err);
      }
    },
  };
}
