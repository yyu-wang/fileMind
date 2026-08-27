// RulesPage 单元测试（对齐交互原型 §规则编辑）：master-detail 布局、
// 加载态、空态、新建表单、规则选中切换、删除二次确认、错误提示。

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
  mocks.reorderRules.mockResolvedValue({ status: 'ok', data: [rule] });
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

  it('renders header with 新建规则 button (master-detail layout)', async () => {
    renderPage();
    await screen.findByText('PDF 归档');
    expect(screen.getByRole('button', { name: /新建规则/ })).toBeInTheDocument();
    expect(screen.getByText('分类规则与分类体系管理')).toBeInTheDocument();
    // 列表 + 详情布局
    expect(screen.getByText('📋 规则列表')).toBeInTheDocument();
  });

  it('opens new rule form on 新建规则 click', async () => {
    const user = userEvent.setup();
    renderPage();
    await screen.findByText('PDF 归档');
    await user.click(screen.getByRole('button', { name: /新建规则/ }));
    // 新建态：右侧详情显示空表单（h3 标题 = "新建规则"）
    const form = await screen.findByTestId('rule-form');
    expect(form).toBeInTheDocument();
    expect(form.querySelector('h3')).toHaveTextContent('新建规则');
  });

  it('shows rule detail when rule-item clicked', async () => {
    const user = userEvent.setup();
    renderPage();
    await screen.findByText('PDF 归档');
    // 列表中点击规则
    await user.click(screen.getAllByTestId('rule-item')[0]);
    // 右侧详情显示该规则的表单（标题为规则名）
    const form = await screen.findByTestId('rule-form');
    expect(form).toHaveTextContent('PDF 归档');
  });

  it('saves a new rule and closes the form', async () => {
    const user = userEvent.setup();
    renderPage();
    await screen.findByText('PDF 归档');
    await user.click(screen.getByRole('button', { name: /新建规则/ }));
    await user.type(screen.getByLabelText('规则名称'), '新规则');
    await user.type(screen.getByLabelText('匹配模式'), 'doc');
    await user.click(screen.getByRole('button', { name: '保存' }));
    expect(mocks.upsertRule).toHaveBeenCalledWith(
      expect.objectContaining({ name: '新规则', pattern: 'doc' }),
    );
  });

  it('deletes rule only after confirm from detail panel', async () => {
    const user = userEvent.setup();
    const confirmSpy = vi.spyOn(window, 'confirm').mockReturnValue(false);
    renderPage();
    await screen.findByText('PDF 归档');
    // 选中规则 → 详情面板出现删除按钮
    await user.click(screen.getAllByTestId('rule-item')[0]);
    await user.click(screen.getByRole('button', { name: '删除规则' }));
    expect(mocks.deleteRule).not.toHaveBeenCalled();

    confirmSpy.mockReturnValue(true);
    await user.click(screen.getByRole('button', { name: '删除规则' }));
    expect(confirmSpy).toHaveBeenCalledWith('确定删除规则「PDF 归档」？此操作不可撤销。');
    expect(mocks.deleteRule).toHaveBeenCalledWith('r1');
    confirmSpy.mockRestore();
  });

  it('renders rules-empty state when no rule selected', async () => {
    renderPage();
    await screen.findByText('PDF 归档');
    // 未选中时右侧显示空态
    expect(screen.getByText('选择左侧规则查看详情')).toBeInTheDocument();
  });
});
