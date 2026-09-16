// OfflineModelSection 单元测试：导入入口、取消不触发、结果/失败文案与导入中禁用。
//
// 策略：mock plugin-dialog 的 open（原生对话框在测试环境点不到）与 store 的导入动作，
// 只断言用户可见行为（文案 / testid / 按钮禁用），不触碰后端与实现细节。

import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';

const mocks = vi.hoisted(() => ({ open: vi.fn() }));

vi.mock('@tauri-apps/plugin-dialog', () => ({ open: mocks.open }));
vi.mock('@/lib/ipc', () => ({ fileIpc: {} }));

import { useSettingsStore } from '@/stores/settingsStore';

import { OfflineModelSection } from './OfflineModelSection';

const importModelPackage = vi.fn(async () => {});

/** 隔离存储：动作替换为桩，避免测试触达真实 IPC。 */
function seed(overrides: Partial<Parameters<typeof useSettingsStore.setState>[0]> = {}): void {
  localStorage.clear();
  mocks.open.mockReset();
  importModelPackage.mockClear();
  useSettingsStore.setState({
    importingPackage: false,
    importResult: null,
    importError: null,
    importModelPackage,
    ...overrides,
  });
}

beforeEach(() => seed());

describe('OfflineModelSection', () => {
  it('explains the offline package requirement', () => {
    render(<OfflineModelSection />);
    expect(screen.getByText(/离线模型包（内网部署）/)).toBeInTheDocument();
    // 包内需要的模型目录名是用户自检包内容是否正确的唯一依据
    expect(screen.getByText(/bge-large-zh-v1.5/)).toBeInTheDocument();
    expect(screen.getByText(/bge-reranker-v2-m3/)).toBeInTheDocument();
  });

  it('imports the selected zip package', async () => {
    mocks.open.mockResolvedValue('/tmp/models.zip');
    render(<OfflineModelSection />);

    fireEvent.click(screen.getByTestId('offline-import-zip-btn'));

    await waitFor(() => expect(importModelPackage).toHaveBeenCalledWith('/tmp/models.zip'));
  });

  it('imports the selected model directory', async () => {
    mocks.open.mockResolvedValue('/tmp/models');
    render(<OfflineModelSection />);

    fireEvent.click(screen.getByTestId('offline-import-dir-btn'));

    await waitFor(() => expect(importModelPackage).toHaveBeenCalledWith('/tmp/models'));
  });

  it('does not call backend when the dialog is cancelled', async () => {
    mocks.open.mockResolvedValue(null);
    render(<OfflineModelSection />);

    fireEvent.click(screen.getByTestId('offline-import-dir-btn'));

    await waitFor(() => expect(mocks.open).toHaveBeenCalled());
    expect(importModelPackage).not.toHaveBeenCalled();
  });

  it('lists imported and skipped models after a successful import', () => {
    seed({
      importResult: {
        imported: ['bge-large-zh-v1.5'],
        skipped: ['bge-reranker-v2-m3'],
      },
    });
    render(<OfflineModelSection />);

    const result = screen.getByTestId('offline-import-result');
    expect(result).toHaveTextContent('已导入：bge-large-zh-v1.5');
    expect(result).toHaveTextContent('已就绪跳过：bge-reranker-v2-m3');
  });

  it('reports the all-ready case when nothing needed importing', () => {
    seed({ importResult: { imported: [], skipped: [] } });
    render(<OfflineModelSection />);

    expect(screen.getByTestId('offline-import-result')).toHaveTextContent('包内模型均已在本机就绪');
  });

  it('shows the backend error verbatim (error code + missing files)', () => {
    seed({ importError: 'EMB-V-001: 包内未找到模型目录，缺少文件：bge-large-zh-v1.5/model.onnx' });
    render(<OfflineModelSection />);

    const alert = screen.getByTestId('offline-import-error');
    expect(alert).toHaveTextContent('EMB-V-001');
    expect(alert).toHaveTextContent('bge-large-zh-v1.5/model.onnx');
    expect(alert).toHaveAttribute('role', 'alert');
  });

  it('ignores the unrelated probe error in the shared error slot', () => {
    // 回归：通用 error 由 Ollama 探测写入，未导入时不能把它显示成「导入失败」
    seed({ error: 'OLLAMA_UNAVAILABLE: Ollama 探测失败' });
    render(<OfflineModelSection />);

    expect(screen.queryByTestId('offline-import-error')).not.toBeInTheDocument();
  });

  it('disables both entries while an import is running', () => {
    seed({ importingPackage: true });
    render(<OfflineModelSection />);

    const zipButton = screen.getByTestId('offline-import-zip-btn');
    const dirButton = screen.getByTestId('offline-import-dir-btn');
    expect(zipButton).toBeDisabled();
    expect(dirButton).toBeDisabled();
    expect(zipButton).toHaveTextContent('导入中…');
    expect(dirButton).toHaveTextContent('导入中…');
  });
});
