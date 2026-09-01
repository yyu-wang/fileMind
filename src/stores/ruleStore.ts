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
import { useClassifyStore } from './classifyStore';

interface RuleState {
  /** 全部规则（含禁用），按优先级降序 */
  rules: Rule[];
  /** 目标分类列表（下拉用） */
  categories: Category[];
  /** 加载中 */
  isLoading: boolean;
  /** 错误信息 */
  error: string | null;
  /** FE-M4：在途 toggle 的规则 id 集合（防连点重复提交） */
  togglingIds: string[];

  /** 加载规则 + 分类 */
  load: () => Promise<void>;
  /** 新建（id 空）或更新规则。返回保存后的 Rule（含后端生成的 id）。 */
  saveRule: (rule: Rule) => Promise<Rule>;
  /** 删除规则 */
  deleteRule: (id: string) => Promise<void>;
  /** 拖拽排序后重排优先级 */
  reorder: (orderedIds: string[]) => Promise<void>;
  /** FE-M4：切换规则启用态（读最新值 + 在途防抖 + 失败回滚） */
  toggleRule: (id: string) => Promise<void>;
  /** 清除错误 */
  clearError: () => void;
}

export const useRuleStore = create<RuleState>()((set, get) => ({
  rules: [],
  categories: [],
  isLoading: false,
  error: null,
  togglingIds: [],

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
      // 规则变化会改变后端分类集合：同步失效 classifyStore 的幂等缓存，
      // 否则分类页手动分类下拉会显示过期分类（双缓存不一致）
      void useClassifyStore.getState().refreshCategories();
      return result.data;
    }
    set({ error: result.error });
    throw new Error(result.error);
  },

  deleteRule: async (id) => {
    set({ error: null });
    const result = await fileIpc.deleteRule(id);
    if (result.status === 'ok') {
      await useRuleStore.getState().load();
      void useClassifyStore.getState().refreshCategories();
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

  // FE-M4：toggle 专用路径。此前 RulesPage.handleToggle 基于渲染闭包里的旧
  // rule 对象调 saveRule——快速连点时两次都发同一旧值，后到响应覆盖前者，
  // 最终启用态与用户所见相反。这里：
  // 1) get() 读最新 rules（不依赖渲染闭包快照）
  // 2) 在途防抖：同一规则的上一次 toggle 未完成时直接忽略本次点击
  //    （开关语义下「忽略在途点击」与「最终态=最后点击」等价）
  // 3) 乐观翻转，失败回滚
  toggleRule: async (id) => {
    if (get().togglingIds.includes(id)) return;
    const current = get().rules.find((r) => r.id === id);
    if (!current) return;
    const prevRules = get().rules;
    set({
      error: null,
      togglingIds: [...get().togglingIds, id],
      rules: prevRules.map((r) => (r.id === id ? { ...r, is_enabled: !r.is_enabled } : r)),
    });
    try {
      const result = await fileIpc.upsertRule({
        ...current,
        is_enabled: !current.is_enabled,
      });
      if (result.status === 'error') throw new Error(result.error);
    } catch (e) {
      // 失败回滚乐观翻转，错误进 store（RulesPage 已有 error 横幅展示）
      set({ rules: prevRules, error: e instanceof Error ? e.message : String(e) });
    } finally {
      set({ togglingIds: get().togglingIds.filter((x) => x !== id) });
    }
  },

  clearError: () => set({ error: null }),
}));
