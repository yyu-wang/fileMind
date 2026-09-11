// ClassifyStatsPanel 单元测试：统计条占比、AI 判断计数、根目录名与目标结构树。

import { beforeEach, describe, expect, it } from 'vitest';
import { render, screen } from '@testing-library/react';
import type { ClassifyPlanItem, ClassifyPreview } from '@/types/ipc';
import { useFileStore } from '@/stores/fileStore';

import { ClassifyStatsPanel } from './ClassifyStatsPanel';

function item(overrides: Partial<ClassifyPlanItem> = {}): ClassifyPlanItem {
  return {
    file_id: 'f1',
    file_name: 'a.pdf',
    original_path: '/root/a.pdf',
    target_path: '/root/文档/a.pdf',
    category_name: '文档',
    rule_source: 'rule:PDF',
    status: 'Ok',
    conflict_type: null,
    ...overrides,
  };
}

const preview: ClassifyPreview = {
  batch_id: 'b1',
  output_root: '/Users/test/我的文档_已分类',
  stats: { total: 10, categorized: 8, pending: 3, by_rule: 5, by_heuristic: 2 },
  items: [
    item({ category_name: '文档', rule_source: 'rule:PDF' }),
    item({ category_name: '文档', rule_source: 'heuristic' }),
    item({ category_name: '图片', rule_source: 'llm', file_id: 'f2', file_name: 'b.png' }),
    item({ category_name: null, status: 'Ok' }),
    item({ category_name: '文档', status: 'Conflict' }),
  ],
};

beforeEach(() => {
  localStorage.clear();
  useFileStore.setState({ scanPath: null });
});

describe('ClassifyStatsPanel', () => {
  it('renders stat bars with rule/heuristic/llm/pending counts and percentages', () => {
    render(<ClassifyStatsPanel preview={preview} />);
    // 统计从 items 派生：规则命中 2 / 总数 5 → 40%
    expect(screen.getByText('规则命中')).toBeInTheDocument();
    expect(screen.getByText('2 (40%)')).toBeInTheDocument();
    // heuristic 1 / 总数 5 → 20%
    expect(screen.getByText('按类型识别')).toBeInTheDocument();
    // llm = items 中 rule_source==='llm' → 1 → 20%
    expect(screen.getByText('AI 判断')).toBeInTheDocument();
    expect(screen.getAllByText('1 (20%)')).toHaveLength(3);
    // pending 1 / 总数 5 → 20%
    expect(screen.getByText('待确认')).toBeInTheDocument();
    expect(screen.getAllByText('1 (20%)')).toHaveLength(3);
  });

  it('derives root name from output_root (收纳根优先)', () => {
    render(<ClassifyStatsPanel preview={preview} />);
    expect(screen.getByText(/我的文档_已分类\/$/)).toBeInTheDocument();
  });

  it('falls back to scanPath root name when output_root absent', () => {
    useFileStore.setState({ scanPath: '/Users/test/我的文档' });
    render(<ClassifyStatsPanel preview={{ ...preview, output_root: '' }} />);
    expect(screen.getByText(/我的文档\/$/)).toBeInTheDocument();
  });

  it('falls back to 文件库 when both roots empty', () => {
    render(<ClassifyStatsPanel preview={{ ...preview, output_root: '' }} />);
    expect(screen.getByText(/文件库\/$/)).toBeInTheDocument();
  });

  it('renders deduplicated target tree excluding conflicts and uncategorized', () => {
    render(<ClassifyStatsPanel preview={preview} />);
    // 文档 ×2 去重为 1，图片 1，Conflict 与 null 排除 → 共 2 个子节点
    expect(screen.getAllByText(/^├── 📁 /)).toHaveLength(2);
    expect(screen.getByText(/文档\/$/)).toBeInTheDocument();
    expect(screen.getByText(/图片\/$/)).toBeInTheDocument();
  });

  it('sorts target tree zh-Hans-CN', () => {
    const mixed: ClassifyPreview = {
      ...preview,
      items: [
        item({ category_name: 'b', rule_source: 'llm' }),
        item({ category_name: 'a', rule_source: 'llm', file_id: 'f2' }),
      ],
    };
    render(<ClassifyStatsPanel preview={mixed} />);
    const children = screen
      .getAllByText(/^├── 📁 /)
      .map((el) => el.textContent?.replace('├── 📁 ', '').replace('/', ''));
    expect(children).toEqual(['a', 'b']);
  });

  it('shows empty tree hint when nothing classifiable', () => {
    const empty: ClassifyPreview = {
      ...preview,
      stats: { total: 0, categorized: 0, pending: 0, by_rule: 0, by_heuristic: 0 },
      items: [item({ category_name: null })],
    };
    render(<ClassifyStatsPanel preview={empty} />);
    expect(screen.getByText('├── （无可分类文件）')).toBeInTheDocument();
  });
});
