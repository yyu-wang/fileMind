// classifyView 单元测试：预览计数、开始按钮文案、页面渲染模型的各状态组合。

import { describe, expect, it } from 'vitest';

import {
  countPreviewItems,
  resolveClassifyPageView,
  resolveStartButtonLabel,
  type ClassifyPageViewInput,
} from './classifyView';
import type { ClassifyPreview, ClassifyPlanItem } from '@/types/ipc';
import { ClassifyStatus } from '@/types/models';

/** 构造预览项，默认是「可执行」项。 */
function planItem(overrides: Partial<ClassifyPlanItem> = {}): ClassifyPlanItem {
  return {
    file_id: 'f1',
    file_name: 'a.pdf',
    original_path: '/tmp/a.pdf',
    target_path: '/tmp/已分类/文档/a.pdf',
    category_name: '文档',
    rule_source: 'heuristic',
    status: 'Ok',
    conflict_type: null,
    ...overrides,
  };
}

/** 构造预览结果。 */
function preview(overrides: Partial<ClassifyPreview> = {}): ClassifyPreview {
  return {
    batch_id: 'b1',
    output_root: '/tmp/已分类',
    items: [planItem()],
    stats: { total: 1, categorized: 1, pending: 0, by_rule: 0, by_heuristic: 1 },
    ...overrides,
  };
}

describe('countPreviewItems', () => {
  it('无预览时计数为 0', () => {
    expect(countPreviewItems(null)).toEqual({ conflictCount: 0, pendingCount: 0 });
  });

  it('统计 Conflict 项与 stats.pending', () => {
    const counts = countPreviewItems(
      preview({
        items: [planItem(), planItem({ status: 'Conflict' }), planItem({ status: 'Conflict' })],
        stats: { total: 3, categorized: 2, pending: 2, by_rule: 1, by_heuristic: 1 },
      }),
    );
    expect(counts).toEqual({ conflictCount: 2, pendingCount: 2 });
  });
});

describe('resolveStartButtonLabel', () => {
  const base = {
    hasFiles: true,
    hasSelection: false,
    selectedCount: 0,
    noUnorganizedTargets: false,
  };

  it('有选中时展示选中数量', () => {
    expect(resolveStartButtonLabel({ ...base, hasSelection: true, selectedCount: 3 })).toBe(
      '对选中的 3 个文件开始分类',
    );
  });

  it('无未整理文件时提示（选中优先于该分支）', () => {
    expect(resolveStartButtonLabel({ ...base, noUnorganizedTargets: true })).toBe(
      '没有未整理的文件',
    );
  });

  it('有文件但无选中时展示全部分类', () => {
    expect(resolveStartButtonLabel(base)).toBe('全部分类');
  });

  it('无文件时引导先扫描', () => {
    expect(resolveStartButtonLabel({ ...base, hasFiles: false })).toBe('请先在文件页扫描目录');
  });
});

describe('resolveClassifyPageView', () => {
  const base: ClassifyPageViewInput = {
    status: ClassifyStatus.Idle,
    preview: null,
    execSummary: null,
    hasUndoBatch: false,
    fileCounts: { total: 0, selected: 0, unorganized: 0 },
  };

  it('初始态（Idle 且无预览）渲染介绍区', () => {
    const view = resolveClassifyPageView(base);
    expect(view.idle).toBe(true);
    expect(view.layoutPreview).toBeNull();
    expect(view.loading).toBe(false);
    expect(view.showHeaderActions).toBe(false);
  });

  it('有预览的 Idle 态渲染双栏并显示头部按钮', () => {
    const view = resolveClassifyPageView({ ...base, preview: preview() });
    expect(view.idle).toBe(false);
    expect(view.layoutPreview).not.toBeNull();
    expect(view.showPreviewSubtitle).toBe(true);
    expect(view.showHeaderActions).toBe(true);
    expect(view.showWarning).toBe(false);
  });

  it('Previewing 态只显示加载提示', () => {
    const view = resolveClassifyPageView({ ...base, status: ClassifyStatus.Previewing });
    expect(view.loading).toBe(true);
    expect(view.layoutPreview).toBeNull();
    expect(view.idle).toBe(false);
  });

  it('执行中隐藏头部按钮但仍渲染布局（供进度遮罩覆盖）', () => {
    const view = resolveClassifyPageView({
      ...base,
      status: ClassifyStatus.Running,
      preview: preview(),
    });
    expect(view.executing).toBe(true);
    expect(view.showHeaderActions).toBe(false);
    expect(view.layoutPreview).not.toBeNull();
    expect(view.idle).toBe(false);
  });

  it('存在冲突/待确认项时显示警告条', () => {
    const view = resolveClassifyPageView({
      ...base,
      preview: preview({
        items: [planItem({ status: 'Conflict' })],
        stats: { total: 1, categorized: 0, pending: 1, by_rule: 0, by_heuristic: 0 },
      }),
    });
    expect(view.showWarning).toBe(true);
    expect(view.conflictCount).toBe(1);
    expect(view.pendingCount).toBe(1);
  });

  it('Done 且有汇总时渲染结果面板，不再渲染双栏', () => {
    const view = resolveClassifyPageView({
      ...base,
      status: ClassifyStatus.Done,
      preview: preview(),
      execSummary: { success: 1, failed: 0, pending: 0, total: 1 },
      hasUndoBatch: true,
    });
    expect(view.layoutPreview).toBeNull();
    expect(view.doneSummary).toEqual({ success: 1, failed: 0, pending: 0, total: 1 });
    expect(view.canUndo).toBe(true);
    expect(view.cancelled).toBe(false);
  });

  it('Cancelled 标记取消态；无汇总时不渲染结果面板', () => {
    const view = resolveClassifyPageView({ ...base, status: ClassifyStatus.Cancelled });
    expect(view.cancelled).toBe(true);
    expect(view.doneSummary).toBeNull();
    expect(view.layoutPreview).toBeNull();
  });

  it('开始按钮：无文件时禁用并引导先扫描', () => {
    const view = resolveClassifyPageView(base);
    expect(view.startButton).toEqual({ label: '请先在文件页扫描目录', disabled: true });
  });

  it('开始按钮：有未整理文件时可全部分类', () => {
    const view = resolveClassifyPageView({
      ...base,
      fileCounts: { total: 2, selected: 0, unorganized: 2 },
    });
    expect(view.startButton).toEqual({ label: '全部分类', disabled: false });
  });

  it('开始按钮：全部已整理且无选中时禁用并提示', () => {
    const view = resolveClassifyPageView({
      ...base,
      fileCounts: { total: 2, selected: 0, unorganized: 0 },
    });
    expect(view.startButton).toEqual({ label: '没有未整理的文件', disabled: true });
  });

  it('开始按钮：有选中时按选中数量展示且可用', () => {
    const view = resolveClassifyPageView({
      ...base,
      fileCounts: { total: 2, selected: 1, unorganized: 0 },
    });
    expect(view.startButton).toEqual({ label: '对选中的 1 个文件开始分类', disabled: false });
  });
});
