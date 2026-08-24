// SearchStatusBar 单元测试：检索/生成阶段、候选数、改写查询、重试与低置信度。

import { describe, expect, it } from 'vitest';
import { render, screen } from '@testing-library/react';
import type { ChatSearchInfo } from '@/stores/chatStore';

import { SearchStatusBar } from './SearchStatusBar';

const base = {
  status: 'searching' as const,
  rewrittenQuery: null as string | null,
  searchInfo: null as ChatSearchInfo | null,
  retries: 0,
  retryReason: null as string | null,
  lowConfidence: false,
  hasTokens: false,
};

describe('SearchStatusBar', () => {
  it('returns null when idle', () => {
    const { container } = render(<SearchStatusBar {...base} status="idle" />);
    expect(container).toBeEmptyDOMElement();
  });

  it('shows searching phase when no tokens yet', () => {
    render(<SearchStatusBar {...base} />);
    expect(screen.getByText('正在检索…')).toBeInTheDocument();
  });

  it('shows generating phase once tokens arrive', () => {
    render(<SearchStatusBar {...base} hasTokens />);
    expect(screen.getByText('正在生成…')).toBeInTheDocument();
  });

  it('renders candidate and after-rerank counts', () => {
    render(
      <SearchStatusBar {...base} searchInfo={{ candidates: 40, afterRerank: 5, sources: [] }} />,
    );
    expect(screen.getByText(/检索到 40 个候选 · 重排后 5 条/)).toBeInTheDocument();
  });

  it('renders rewritten query', () => {
    render(<SearchStatusBar {...base} rewrittenQuery="年度总结" />);
    expect(screen.getByText('改写查询: 年度总结')).toBeInTheDocument();
  });

  it('renders retry count and reason', () => {
    render(<SearchStatusBar {...base} retries={2} retryReason="缺少引用" />);
    expect(screen.getByText(/自我纠正 2 次：缺少引用/)).toBeInTheDocument();
  });

  it('renders retry count without reason', () => {
    render(<SearchStatusBar {...base} retries={1} />);
    expect(screen.getByText('自我纠正 1 次')).toBeInTheDocument();
  });

  it('renders low-confidence warning', () => {
    render(<SearchStatusBar {...base} lowConfidence />);
    expect(screen.getByText('低置信度')).toBeInTheDocument();
  });
});
