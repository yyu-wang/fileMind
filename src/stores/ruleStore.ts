// 规则 store：分类规则列表 + 目标分类 + CRUD + 优先级拖拽排序。
//
// 不持久化：规则数据以 SQLite 为唯一事实来源，启动时从 Rust 拉取。
//
// IPC 调用：
// - listRules() → Rule[]
// - listCategories() → Category[]
// - upsertRule(rule) → Rule（id 为空则后端生成 UUID）
// - deleteRule(id) → null
// - reorderRules(orderedIds) → Rule[]

import { create } from 'zustand';
import { fileIpc } from '../lib/ipc';
import type { Category, Rule } from '../types/ipc';

interface RuleState {
  /** 全部规则（含禁用），按优先级降序 */
  rules: Rule[];
  /** 目标分类列表（下拉用） */
  categories: Category[];
  /** 加载中 */
  isLoading: boolean;
  /** 错误信息 */
  error: string | null;

  /** 加载规则 + 分类 */
  load: () => Promise<void>;
  /** 新建（id 空）或更新规则 */
  saveRule: (rule: Rule) => Promise<void>;
  /** 删除规则 */
  deleteRule: (id: string) => Promise<void>;
  /** 拖拽排序后重排优先级 */
  reorder: (orderedIds: string[]) => Promise<void>;
  /** 清除错误 */
  clearError: () => void;
}

export const useRuleStore = create<RuleState>()((set) => ({
  rules: [],
  categories: [],
  isLoading: false,
  error: null,

  load: async () => {
    set({ isLoading: true, error: null });
    const [rulesRes, catsRes] = await Promise.all([fileIpc.listRules(), fileIpc.listCategories()]);
    if (rulesRes.status === 'ok' && catsRes.status === 'ok') {
      set({ rules: rulesRes.data, categories: catsRes.data, isLoading: false });
    } else {
      // 至少一个失败：分别收窄类型拿到错误信息
      let err = '规则或分类加载失败';
      if (rulesRes.status === 'error') {
        err = rulesRes.error;
      } else if (catsRes.status === 'error') {
        err = catsRes.error;
      }
      set({ isLoading: false, error: err });
    }
  },

  saveRule: async (rule) => {
    set({ error: null });
    const result = await fileIpc.upsertRule(rule);
    if (result.status === 'ok') {
      // 保存后重新拉取，确保 created_at/updated_at 与优先级排序一致
      await useRuleStore.getState().load();
    } else {
      set({ error: result.error });
      throw new Error(result.error);
    }
  },

  deleteRule: async (id) => {
    set({ error: null });
    const result = await fileIpc.deleteRule(id);
    if (result.status === 'ok') {
      await useRuleStore.getState().load();
    } else {
      set({ error: result.error });
      throw new Error(result.error);
    }
  },

  reorder: async (orderedIds) => {
    set({ error: null });
    const result = await fileIpc.reorderRules(orderedIds);
    if (result.status === 'ok') {
      set({ rules: result.data });
    } else {
      set({ error: result.error });
      throw new Error(result.error);
    }
  },

  clearError: () => set({ error: null }),
}));
