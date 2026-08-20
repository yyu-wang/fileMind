// 全局错误边界（T6.10）：捕获子树渲染错误，避免整棵 React 树卸载白屏。
//
// React 错误边界必须是 class 组件（getDerivedStateFromError / componentDidCatch
// 仅 class 可用），捕获错误后用 ErrorState 展示并提供「重新加载」重置。

import { Component, type ErrorInfo, type ReactNode } from 'react';
import { ErrorState } from './StateViews';

interface ErrorBoundaryProps {
  children: ReactNode;
}

interface ErrorBoundaryState {
  error: Error | null;
}

export class ErrorBoundary extends Component<ErrorBoundaryProps, ErrorBoundaryState> {
  state: ErrorBoundaryState = { error: null };

  static getDerivedStateFromError(error: Error): ErrorBoundaryState {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo): void {
    console.error('[ErrorBoundary] 未捕获的渲染错误:', error, info.componentStack);
  }

  render(): ReactNode {
    if (this.state.error) {
      return (
        <ErrorState
          title="应用出现错误"
          description={this.state.error.message}
          onRetry={() => this.setState({ error: null })}
        />
      );
    }
    return this.props.children;
  }
}
