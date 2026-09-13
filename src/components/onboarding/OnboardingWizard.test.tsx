// OnboardingWizard 集成测试：三步流转（mode → consent → directory）+ 错误路径。
//
// 策略：mock fileIpc（走真实 store action 验证数据流）、mock plugin-dialog 的 open、
// mock ConsentAgreement 为可控组件绕过滚动门控（与 StepConsent.test 相同策略）。
// 覆盖：Local 直通 directory、Cloud 经 consent、目录浏览/E2E 自动填充、
// IPC 失败时的错误横幅与关闭。

import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import type { CloudProviderRecord, FileInfo } from '@/types/ipc';

import { useFileStore } from '@/stores/fileStore';
import { useSettingsStore } from '@/stores/settingsStore';
import { OnboardingWizard } from './OnboardingWizard';

const mocks = vi.hoisted(() => ({
  ollamaStatus: vi.fn(),
  setInferenceMode: vi.fn(),
  signCloudConsent: vi.fn(),
  updateConfig: vi.fn(),
  scanDirectory: vi.fn(),
  getFileStats: vi.fn(),
  e2eGetTestDir: vi.fn(),
  open: vi.fn(),
}));

vi.mock('@/lib/ipc', () => ({
  fileIpc: {
    ollamaStatus: mocks.ollamaStatus,
    setInferenceMode: mocks.setInferenceMode,
    signCloudConsent: mocks.signCloudConsent,
    updateConfig: mocks.updateConfig,
    scanDirectory: mocks.scanDirectory,
    getFileStats: mocks.getFileStats,
    e2eGetTestDir: mocks.e2eGetTestDir,
  },
}));

vi.mock('@tauri-apps/plugin-dialog', () => ({ open: mocks.open }));

vi.mock('../consent/ConsentAgreement', () => ({
  ConsentAgreement: (props: { version: string; onBottomReached: (reached: boolean) => void }) => (
    <div>
      <button type="button" onClick={() => props.onBottomReached(true)}>
        scroll-to-bottom
      </button>
    </div>
  ),
}));

const OK_OLLAMA = {
  status: 'ok',
  data: {
    available: true,
    status: 'ok',
    llm_models: [],
    embedding_models: [],
    error_code: null,
    message: null,
  },
};

// P-07：consent 步骤默认选中 provider = cloudProviders[0]，seed 使默认键为 'Openai'
//（signCloudConsent 断言沿用既有大小写约定）
const builtinCloudProviders: CloudProviderRecord[] = [
  {
    id: 'builtin-openai',
    provider_key: 'Openai',
    name: 'OpenAI',
    remark: '',
    website: null,
    base_url: '',
    is_builtin: true,
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
  },
  {
    id: 'builtin-deepseek',
    provider_key: 'Deepseek',
    name: 'DeepSeek',
    remark: '',
    website: null,
    base_url: '',
    is_builtin: true,
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
  },
];

function scannedFile(): FileInfo {
  return {
    id: 'f1',
    path: '/tmp/docs/a.md',
    file_name: 'a.md',
    file_size: 10,
    content_hash: null,
    category: null,
    created_at: '2026-08-01 00:00:00',
    updated_at: '2026-08-01 00:00:00',
  };
}

beforeEach(() => {
  localStorage.clear();
  vi.clearAllMocks();
  mocks.ollamaStatus.mockResolvedValue(OK_OLLAMA);
  mocks.setInferenceMode.mockResolvedValue({ status: 'ok', data: null });
  mocks.signCloudConsent.mockResolvedValue({ status: 'ok', data: null });
  mocks.updateConfig.mockResolvedValue({ status: 'ok', data: null });
  mocks.scanDirectory.mockResolvedValue({ status: 'ok', data: [scannedFile()] });
  mocks.getFileStats.mockResolvedValue({ status: 'ok', data: { total_files: 1 } });
  mocks.e2eGetTestDir.mockResolvedValue(null);
  // 复位 store（模块级单例跨测试残留）
  useSettingsStore.setState({
    ollamaStatus: null,
    ollamaProbing: false,
    lastOllamaProbeAt: 0,
    error: null,
    onboardingCompleted: false,
    dataDirectory: '',
    cloudProviders: builtinCloudProviders,
    cloudProvidersLoading: false,
  });
  useFileStore.setState({
    files: [],
    scanPath: null,
    isScanning: false,
    selectedIds: [],
    error: null,
    total: 0,
  });
});

describe('OnboardingWizard step 1（模式选择）', () => {
  it('renders two mode options with Local preselected and Ollama detected callout', async () => {
    render(<OnboardingWizard />);
    expect(screen.getByText('选择你的 AI 推理模式')).toBeInTheDocument();
    await waitFor(() => {
      expect(screen.getByText(/已检测到 Ollama/)).toBeInTheDocument();
    });
    expect(screen.getByRole('radio', { name: /本地模式/ })).toHaveAttribute('aria-checked', 'true');
    expect(screen.getByRole('radio', { name: /云端模式/ })).toHaveAttribute(
      'aria-checked',
      'false',
    );
    // P1 未开发功能直接隐藏：不渲染混合模式选项
    expect(screen.queryByRole('radio', { name: /混合模式/ })).not.toBeInTheDocument();
  });

  it('shows warning callout when Ollama unavailable', async () => {
    mocks.ollamaStatus.mockResolvedValue({
      status: 'ok',
      data: { ...OK_OLLAMA.data, available: false, status: 'unavailable' },
    });
    render(<OnboardingWizard />);
    await waitFor(() => {
      expect(screen.getByText(/未检测到本地 Ollama/)).toBeInTheDocument();
    });
  });

  it('shows probing callout while the probe is in flight', () => {
    mocks.ollamaStatus.mockImplementation(() => new Promise(() => undefined));
    render(<OnboardingWizard />);
    expect(screen.getByText(/正在检测本地 Ollama/)).toBeInTheDocument();
  });

  it('switches selection on option click (Cloud)', async () => {
    const user = userEvent.setup();
    render(<OnboardingWizard />);
    await user.click(screen.getByRole('radio', { name: /云端模式/ }));
    expect(screen.getByRole('radio', { name: /云端模式/ })).toHaveAttribute('aria-checked', 'true');
    expect(screen.getByRole('radio', { name: /本地模式/ })).toHaveAttribute(
      'aria-checked',
      'false',
    );
  });
});

describe('OnboardingWizard 步骤流转', () => {
  it('Local → skips consent and goes straight to directory step', async () => {
    const user = userEvent.setup();
    render(<OnboardingWizard />);
    await user.click(screen.getByTestId('onboarding-next'));
    expect(mocks.setInferenceMode).toHaveBeenCalledWith('Local', 'ui');
    expect(screen.getByText('选择要管理的目录')).toBeInTheDocument();
    expect(screen.getByText(/将完全在本地处理/)).toBeInTheDocument();
  });

  it('Cloud → consent step → confirm signs consent and enters directory', async () => {
    const user = userEvent.setup();
    render(<OnboardingWizard />);
    await user.click(screen.getByRole('radio', { name: /云端模式/ }));
    await user.click(screen.getByTestId('onboarding-next'));
    // consent 步骤出现
    expect(screen.getByRole('checkbox')).toBeInTheDocument();
    fireEvent.click(screen.getByText('scroll-to-bottom'));
    await user.click(screen.getByRole('checkbox'));
    await user.click(screen.getByRole('button', { name: '确认并继续' }));
    expect(mocks.signCloudConsent).toHaveBeenCalledWith(expect.anything(), 'Openai');
    expect(screen.getByText('选择要管理的目录')).toBeInTheDocument();
    expect(screen.getByText(/内容摘要将上传到云端处理/)).toBeInTheDocument();
  });

  it('consent back button returns to mode step', async () => {
    const user = userEvent.setup();
    render(<OnboardingWizard />);
    await user.click(screen.getByRole('radio', { name: /云端模式/ }));
    await user.click(screen.getByTestId('onboarding-next'));
    await user.click(screen.getByRole('button', { name: /返回选其他模式/ }));
    expect(screen.getByText('选择你的 AI 推理模式')).toBeInTheDocument();
  });
});

describe('OnboardingWizard directory 步骤', () => {
  async function goDirectory(user: ReturnType<typeof userEvent.setup>): Promise<void> {
    render(<OnboardingWizard />);
    await user.click(screen.getByTestId('onboarding-next'));
  }

  it('browse selects a directory and 开始使用 completes onboarding + first scan', async () => {
    const user = userEvent.setup();
    await goDirectory(user);
    mocks.open.mockResolvedValue('/tmp/docs');

    // 初始：dropzone 展示、开始使用禁用
    expect(screen.getByText('拖拽文件夹到此处')).toBeInTheDocument();
    expect(screen.getByTestId('onboarding-start')).toBeDisabled();

    await user.click(screen.getByText('拖拽文件夹到此处'));
    expect(mocks.open).toHaveBeenCalledWith(
      expect.objectContaining({ directory: true, multiple: false }),
    );
    expect(screen.getByText('/tmp/docs')).toBeInTheDocument();
    expect(screen.getByTestId('onboarding-start')).toBeEnabled();

    await user.click(screen.getByTestId('onboarding-start'));
    await waitFor(() => {
      expect(mocks.updateConfig).toHaveBeenCalledWith(
        expect.objectContaining({ data_directory: '/tmp/docs', onboarding_completed: true }),
      );
    });
    expect(mocks.scanDirectory).toHaveBeenCalledWith('/tmp/docs');
  });

  it('auto-fills the E2E test dir and skips the dropzone', async () => {
    const user = userEvent.setup();
    mocks.e2eGetTestDir.mockResolvedValue('/e2e/dir');
    render(<OnboardingWizard />);
    await user.click(screen.getByTestId('onboarding-next'));
    await waitFor(() => {
      expect(screen.getByText('/e2e/dir')).toBeInTheDocument();
    });
    expect(screen.queryByText('拖拽文件夹到此处')).not.toBeInTheDocument();
    expect(screen.getByTestId('onboarding-start')).toBeEnabled();
  });

  it('cancelling the native dialog keeps directory empty', async () => {
    const user = userEvent.setup();
    await goDirectory(user);
    mocks.open.mockResolvedValue(null);
    await user.click(screen.getByText('拖拽文件夹到此处'));
    expect(screen.getByText('拖拽文件夹到此处')).toBeInTheDocument();
    expect(screen.getByTestId('onboarding-start')).toBeDisabled();
  });
});

describe('OnboardingWizard 错误处理', () => {
  it('shows an error banner when setInferenceMode fails, dismissible', async () => {
    const user = userEvent.setup();
    mocks.setInferenceMode.mockResolvedValue({ status: 'error', error: '模式切换失败' });
    render(<OnboardingWizard />);
    await user.click(screen.getByTestId('onboarding-next'));
    expect(await screen.findByRole('alert')).toHaveTextContent('模式切换失败');
    await user.click(screen.getByRole('button', { name: '关闭错误' }));
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
  });

  it('shows an error banner when consent signing fails', async () => {
    const user = userEvent.setup();
    mocks.signCloudConsent.mockResolvedValue({ status: 'error', error: '同意书签署失败' });
    render(<OnboardingWizard />);
    await user.click(screen.getByRole('radio', { name: /云端模式/ }));
    await user.click(screen.getByTestId('onboarding-next'));
    fireEvent.click(screen.getByText('scroll-to-bottom'));
    await user.click(screen.getByRole('checkbox'));
    await user.click(screen.getByRole('button', { name: '确认并继续' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('同意书签署失败');
  });

  it('shows an error banner when completing onboarding fails', async () => {
    const user = userEvent.setup();
    mocks.updateConfig.mockResolvedValue({ status: 'error', error: '完成引导失败' });
    mocks.e2eGetTestDir.mockResolvedValue('/e2e/dir');
    render(<OnboardingWizard />);
    await user.click(screen.getByTestId('onboarding-next'));
    await waitFor(() => {
      expect(screen.getByTestId('onboarding-start')).toBeEnabled();
    });
    await user.click(screen.getByTestId('onboarding-start'));
    expect(await screen.findByRole('alert')).toHaveTextContent('完成引导失败');
  });
});
