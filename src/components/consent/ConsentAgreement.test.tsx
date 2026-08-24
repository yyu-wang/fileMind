// ConsentAgreement 滚动到底门控测试。
//
// jsdom 默认 scrollHeight=0、clientHeight=600（tests/setup.ts mock），
// 挂载即「无需滚动即到底」；通过 Object.defineProperty 覆写 scrollHeight/
// scrollTop 模拟内容溢出与滚动位置，验证 onBottomReached 上报。

import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { ConsentAgreement } from './ConsentAgreement';

/** 模拟滚动容器：内容高 1000px（clientHeight 恒 600），当前 scrollTop。 */
function setScrollPosition(element: HTMLElement, scrollHeight: number, scrollTop: number) {
  Object.defineProperty(element, 'scrollHeight', { value: scrollHeight, configurable: true });
  Object.defineProperty(element, 'scrollTop', { value: scrollTop, configurable: true });
}

describe('ConsentAgreement', () => {
  it('renders consent version inside the scroll body', () => {
    render(<ConsentAgreement version="v1.0" onBottomReached={vi.fn()} />);
    expect(screen.getByText('同意书版本：v1.0')).toBeInTheDocument();
  });

  it('reports bottom reached when content fits without scrolling', () => {
    const onBottomReached = vi.fn();
    render(<ConsentAgreement version="v1.0" onBottomReached={onBottomReached} />);
    // jsdom 默认尺寸 → scrollHeight(0) - scrollTop(0) - clientHeight(600) ≤ 4
    expect(onBottomReached).toHaveBeenLastCalledWith(true);
  });

  it('reports not reached when content overflows and user has not scrolled', () => {
    const onBottomReached = vi.fn();
    render(<ConsentAgreement version="v1.0" onBottomReached={onBottomReached} />);
    const scroll = screen.getByRole('document');
    setScrollPosition(scroll, 1000, 0); // 1000 - 0 - 600 = 400 > 4
    fireEvent.scroll(scroll);
    expect(onBottomReached).toHaveBeenLastCalledWith(false);
  });

  it('reports bottom reached after scrolling to the end', () => {
    const onBottomReached = vi.fn();
    render(<ConsentAgreement version="v1.0" onBottomReached={onBottomReached} />);
    const scroll = screen.getByRole('document');
    setScrollPosition(scroll, 1000, 400); // 1000 - 400 - 600 = 0 ≤ 4
    fireEvent.scroll(scroll);
    expect(onBottomReached).toHaveBeenLastCalledWith(true);
  });
});
