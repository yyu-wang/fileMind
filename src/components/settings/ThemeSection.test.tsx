// ThemeSection 单元测试：三态主题切换与 aria-pressed 状态。

import { beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';

vi.mock('@/lib/ipc', () => ({ fileIpc: {} }));

import { ThemeMode } from '@/types/models';
import { useSettingsStore } from '@/stores/settingsStore';

import { ThemeSection } from './ThemeSection';

function seed(theme: ThemeMode): void {
  localStorage.clear();
  useSettingsStore.setState({
    inferenceMode: 'Local',
    llmModel: 'qwen3.8-27b',
    dataDirectory: '',
    embeddingModel: 'bge-small-zh',
    maxFileSizeMb: 100,
    language: 'zh-CN',
    onboardingCompleted: false,
    cloudConsentSigned: false,
    cloudConsentVersion: null,
    cloudConsentProvider: null,
    cloudConsentSignedAt: null,
    isLoading: false,
    error: null,
    ollamaStatus: null,
    ollamaProbing: false,
    llmModelOptions: [],
    embeddingModelOptions: [],
    theme,
  });
}

beforeEach(() => seed(ThemeMode.System));

describe('ThemeSection', () => {
  it('renders three theme options with system selected by default', () => {
    render(<ThemeSection />);
    const options = screen.getAllByRole('button', { pressed: true });
    expect(options).toHaveLength(1);
    expect(options[0]).toHaveTextContent('跟随系统');
    expect(screen.getByRole('button', { name: /亮色/ })).toHaveAttribute('aria-pressed', 'false');
  });

  it('switches theme to dark on click and persists in store', async () => {
    const user = userEvent.setup();
    render(<ThemeSection />);
    await user.click(screen.getByRole('button', { name: /暗色/ }));
    expect(useSettingsStore.getState().theme).toBe(ThemeMode.Dark);
    expect(screen.getByRole('button', { name: /暗色/ })).toHaveAttribute('aria-pressed', 'true');
    expect(screen.getByRole('button', { name: /亮色/ })).toHaveAttribute('aria-pressed', 'false');
  });
});
