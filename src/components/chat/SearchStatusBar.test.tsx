// SearchStatusBar 单元测试：检索/生成阶段、候选数、改写查询、重试与低置信度。
// 组件自行订阅 chatStore，用例通过 setState 驱动状态。

import { describe, expect, it } from 'vitest';
import { render, screen } from '@testing-library/react';
import { useChatStore } from '@/stores/chatStore';

import { SearchStatusBar } from './SearchStatusBar';

/** 重置并预置 store 状态（idle 基线）。 */
function setStoreState(partial: Partial<ReturnType<typeof useChatStore.getState>> = {}): void {
  useChatStore.setState({
    status: 'idle',
    rewrittenQuery: null,
    searchInfo: null,
    retries: 0,
    retryReason: null,
    lowConfidence: false,
    currentStream: '',
    ...partial,
  });
}

describe('SearchStatusBar', () => {
  it('returns null when idle', () => {
    setStoreState({ status: 'idle' });
    const { container } = render(<SearchStatusBar />);
    expect(container).toBeEmptyDOMElement();
  });

  it('shows searching phase when no tokens yet', () => {
    setStoreState({ status: 'searching' });
    render(<SearchStatusBar />);
    expect(screen.getByText('正在检索…')).toBeInTheDocument();
  });

  it('shows generating phase once tokens arrive', () => {
    setStoreState({ status: 'streaming', currentStream: '你好' });
    render(<SearchStatusBar />);
    expect(screen.getByText('正在生成…')).toBeInTheDocument();
  });

  it('renders candidate and after-rerank counts', () => {
    setStoreState({
      status: 'searching',
      searchInfo: { candidates: 40, afterRerank: 5, sources: [] },
    });
    render(<SearchStatusBar />);
    expect(screen.getByText(/检索到 40 个候选 · 重排后 5 条/)).toBeInTheDocument();
  });

  it('renders rewritten query', () => {
    setStoreState({ status: 'searching', rewrittenQuery: '年度总结' });
    render(<SearchStatusBar />);
    expect(screen.getByText('改写查询: 年度总结')).toBeInTheDocument();
  });

  it('renders retry count and reason', () => {
    setStoreState({ status: 'streaming', retries: 2, retryReason: '缺少引用' });
    render(<SearchStatusBar />);
    expect(screen.getByText(/自我纠正 2 次：缺少引用/)).toBeInTheDocument();
  });

  it('renders retry count without reason', () => {
    setStoreState({ status: 'streaming', retries: 1 });
    render(<SearchStatusBar />);
    expect(screen.getByText('自我纠正 1 次')).toBeInTheDocument();
  });

  it('renders low-confidence warning', () => {
    setStoreState({ status: 'streaming', lowConfidence: true });
    render(<SearchStatusBar />);
    expect(screen.getByText('低置信度')).toBeInTheDocument();
  });
});
