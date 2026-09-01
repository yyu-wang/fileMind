// ruleStore 单元测试：加载（并行 rules+categories）、CRUD 后重拉取、排序、错误分支。

import { beforeEach, describe, expect, it, vi, type Mock } from 'vitest';

vi.mock('../lib/ipc', () => ({
  fileIpc: {
    listRules: vi.fn(),
    listCategories: vi.fn(),
    upsertRule: vi.fn(),
    deleteRule: vi.fn(),
    reorderRules: vi.fn(),
  },
}));

import { fileIpc } from '../lib/ipc';
import type { Category, Rule } from '../types/ipc';
import { useRuleStore } from './ruleStore';

function rule(id: string, overrides: Partial<Rule> = {}): Rule {
  return {
    id,
    name: '规则 A',
    rule_type: 'extension',
    pattern: 'pdf',
    target_category: 'cat-1',
    priority: 10,
    is_enabled: true,
    created_at: '2026-08-01 00:00:00',
    updated_at: '2026-08-01 00:00:00',
    ...overrides,
  };
}

function category(id: string, name: string): Category {
  return {
    id,
    name,
    parent_id: null,
    icon: null,
    color: null,
    sort_order: 0,
    is_builtin: true,
    target_dir: '',
    created_at: '2026-08-01 00:00:00',
    updated_at: '2026-08-01 00:00:00',
  };
}

const rules = [rule('r1', { priority: 20 }), rule('r2', { priority: 10 })];
const categories = [category('cat-1', '文档')];

function freshState(): void {
  useRuleStore.setState({ rules: [], categories: [], isLoading: false, error: null });
}

describe('ruleStore', () => {
  beforeEach(() => {
    freshState();
    vi.clearAllMocks();
    // 默认：listRules/listCategories 成功，供 save/delete 成功后重拉取
    (fileIpc.listRules as Mock).mockResolvedValue({ status: 'ok', data: rules });
    (fileIpc.listCategories as Mock).mockResolvedValue({ status: 'ok', data: categories });
  });

  it('load 成功：并行拉取 rules + categories', async () => {
    await useRuleStore.getState().load();

    const s = useRuleStore.getState();
    expect(s.isLoading).toBe(false);
    expect(s.rules).toEqual(rules);
    expect(s.categories).toEqual(categories);
    expect(fileIpc.listRules).toHaveBeenCalled();
    expect(fileIpc.listCategories).toHaveBeenCalled();
  });

  it('load 失败：优先展示 rules 错误', async () => {
    (fileIpc.listRules as Mock).mockResolvedValue({ status: 'error', error: 'RULE-E-001' });
    (fileIpc.listCategories as Mock).mockResolvedValue({ status: 'ok', data: categories });

    await useRuleStore.getState().load();

    expect(useRuleStore.getState().isLoading).toBe(false);
    expect(useRuleStore.getState().error).toBe('RULE-E-001');
  });

  it('load 失败：rules 成功但 categories 失败时展示 categories 错误', async () => {
    (fileIpc.listRules as Mock).mockResolvedValue({ status: 'ok', data: rules });
    (fileIpc.listCategories as Mock).mockResolvedValue({ status: 'error', error: 'CAT-E-002' });

    await useRuleStore.getState().load();

    expect(useRuleStore.getState().error).toBe('CAT-E-002');
  });

  it('saveRule 成功：upsert 后重新拉取列表并返回保存后的 Rule', async () => {
    const saved = rule('r3');
    (fileIpc.upsertRule as Mock).mockResolvedValue({ status: 'ok', data: saved });

    const result = await useRuleStore.getState().saveRule(rule('r3', { id: '' }));

    expect(fileIpc.upsertRule).toHaveBeenCalledWith(rule('r3', { id: '' }));
    expect(fileIpc.listRules).toHaveBeenCalled(); // 保存后 reload
    expect(useRuleStore.getState().error).toBeNull();
    expect(result).toEqual(saved);
  });

  it('saveRule 失败：置错并抛出', async () => {
    (fileIpc.upsertRule as Mock).mockResolvedValue({ status: 'error', error: 'RULE-E-003' });

    await expect(useRuleStore.getState().saveRule(rule('r3'))).rejects.toThrow('RULE-E-003');
    expect(useRuleStore.getState().error).toBe('RULE-E-003');
    expect(fileIpc.listRules).not.toHaveBeenCalled();
  });

  it('deleteRule 成功：删除后重新拉取列表', async () => {
    (fileIpc.deleteRule as Mock).mockResolvedValue({ status: 'ok', data: null });

    await useRuleStore.getState().deleteRule('r1');

    expect(fileIpc.deleteRule).toHaveBeenCalledWith('r1');
    expect(fileIpc.listRules).toHaveBeenCalled();
  });

  it('deleteRule 失败：置错并抛出', async () => {
    (fileIpc.deleteRule as Mock).mockResolvedValue({ status: 'error', error: 'RULE-E-004' });

    await expect(useRuleStore.getState().deleteRule('r1')).rejects.toThrow('RULE-E-004');
    expect(useRuleStore.getState().error).toBe('RULE-E-004');
  });

  it('reorder 成功：用后端返回值覆盖规则列表', async () => {
    const reordered = [rule('r2'), rule('r1')];
    (fileIpc.reorderRules as Mock).mockResolvedValue({ status: 'ok', data: reordered });

    await useRuleStore.getState().reorder(['r2', 'r1']);

    expect(fileIpc.reorderRules).toHaveBeenCalledWith(['r2', 'r1']);
    expect(useRuleStore.getState().rules).toEqual(reordered);
  });

  it('reorder 失败：置错并抛出', async () => {
    (fileIpc.reorderRules as Mock).mockResolvedValue({ status: 'error', error: 'RULE-E-005' });

    await expect(useRuleStore.getState().reorder(['r1'])).rejects.toThrow('RULE-E-005');
    expect(useRuleStore.getState().error).toBe('RULE-E-005');
  });

  it('clearError：清除错误', () => {
    useRuleStore.setState({ error: 'boom' });

    useRuleStore.getState().clearError();

    expect(useRuleStore.getState().error).toBeNull();
  });
});
