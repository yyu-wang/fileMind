// RulesPage 单元测试：加载态、列表渲染、空态、新建表单、删除二次确认与错误提示。

import { beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import type { Category, Rule } from '@/types/ipc';
import { RuleType } from '@/types/models';

import { useRuleStore } from '@/stores/ruleStore';
import { RulesPage } from './RulesPage';

const mocks = vi.hoisted(() => ({
  listRules: vi.fn(),
  listCategories: vi.fn(),
  upsertRule: vi.fn(),
  deleteRule: vi.fn(),
  reorderRules: vi.fn(),
}));

vi.mock('@/lib/ipc', () => ({
  fileIpc: {
    listRules: mocks.listRules,
    listCategories: mocks.listCategories,
    upsertRule: mocks.upsertRule,
    deleteRule: mocks.deleteRule,
    reorderRules: mocks.reorderRules,
  },
}));

const category: Category = {
  id: 'c1',
  name: '文档',
  parent_id: null,
  icon: null,
  color: null,
  sort_order: 0,
  is_builtin: true,
  target_dir: '',
  created_at: '',
  updated_at: '',
};

const rule: Rule = {
  id: 'r1',
  name: 'PDF 归档',
  rule_type: RuleType.Extension,
  pattern: 'pdf',
  target_category: 'c1',
  priority: 10,
  is_enabled: true,
  created_at: '',
  updated_at: '',
};

beforeEach(() => {
  localStorage.clear();
  vi.clearAllMocks();
  mocks.listRules.mockResolvedValue({ status: 'ok', data: [rule] });
  mocks.listCategories.mockResolvedValue({ status: 'ok', data: [category] });
  mocks.upsertRule.mockResolvedValue({ status: 'ok', data: rule });
  mocks.deleteRule.mockResolvedValue({ status: 'ok', data: null });
  mocks.reorderRules.mockResolvedValue({ status: 'ok', data: null });
  useRuleStore.setState({
    rules: [],
    categories: [],
    isLoading: false,
    error: null,
  });
});

function renderPage(): void {
  render(<RulesPage />);
}

describe('RulesPage', () => {
  it('loads rules on mount and renders the list', async () => {
    renderPage();
    expect(await screen.findByText('PDF 归档')).toBeInTheDocument();
    expect(mocks.listRules).toHaveBeenCalledTimes(1);
    expect(mocks.listCategories).toHaveBeenCalledTimes(1);
  });

  it('shows loading state while fetching', () => {
    mocks.listRules.mockImplementation(() => new Promise(() => {}));
    mocks.listCategories.mockImplementation(() => new Promise(() => {}));
    renderPage();
    expect(screen.getByText('加载规则…')).toBeInTheDocument();
  });

  it('shows empty state when no rules', async () => {
    mocks.listRules.mockResolvedValue({ status: 'ok', data: [] });
    renderPage();
    expect(await screen.findByText('暂无自定义规则')).toBeInTheDocument();
  });

  it('shows and dismisses load error alert', async () => {
    const user = userEvent.setup();
    mocks.listRules.mockResolvedValue({ status: 'error', error: '规则加载失败' });
    renderPage();
    const alert = await screen.findByRole('alert');
    expect(alert).toHaveTextContent('规则加载失败');
    await user.click(screen.getByLabelText('关闭错误提示'));
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
  });

  it('opens new rule form on 新建规则', async () => {
    const user = userEvent.setup();
    renderPage();
    await screen.findByText('PDF 归档');
    await user.click(screen.getByRole('button', { name: '+ 新建规则' }));
    expect(screen.getByRole('dialog')).toHaveAccessibleName('规则编辑');
    expect(screen.getByRole('dialog')).toHaveTextContent('新建规则');
  });

  it('saves a new rule and closes the form', async () => {
    const user = userEvent.setup();
    renderPage();
    await screen.findByText('PDF 归档');
    await user.click(screen.getByRole('button', { name: '+ 新建规则' }));
    await user.type(screen.getByLabelText('规则名'), '新规则');
    await user.type(screen.getByLabelText('匹配模式'), 'doc');
    await user.click(screen.getByRole('button', { name: '保存' }));
    expect(mocks.upsertRule).toHaveBeenCalledWith(
      expect.objectContaining({ name: '新规则', pattern: 'doc' }),
    );
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
  });

  it('deletes rule only after confirm', async () => {
    const user = userEvent.setup();
    const confirmSpy = vi.spyOn(window, 'confirm').mockReturnValue(false);
    renderPage();
    await screen.findByText('PDF 归档');
    await user.click(screen.getByRole('button', { name: '删除' }));
    expect(mocks.deleteRule).not.toHaveBeenCalled();

    confirmSpy.mockReturnValue(true);
    await user.click(screen.getByRole('button', { name: '删除' }));
    expect(confirmSpy).toHaveBeenCalledWith('确定删除规则「PDF 归档」？此操作不可撤销。');
    expect(mocks.deleteRule).toHaveBeenCalledWith('r1');
    confirmSpy.mockRestore();
  });

  it('toggles rule via checkbox and persists', async () => {
    const user = userEvent.setup();
    renderPage();
    await screen.findByText('PDF 归档');
    await user.click(screen.getByLabelText('禁用规则 PDF 归档'));
    expect(mocks.upsertRule).toHaveBeenCalledWith(
      expect.objectContaining({ id: 'r1', is_enabled: false }),
    );
  });
});
