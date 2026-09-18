// settingsStore 单元测试（模型侧）：离线模型包导入 + 本地生成后端配置（V019）。
//
// 从 settingsStore.test.ts 拆出（原单文件 572 行超测试文件警告阈值 400）；
// 共享夹具与 vi.mock 的说明见该文件与本目录 settingsTestFixtures.ts。

import { beforeEach, describe, expect, it, vi, type Mock } from 'vitest';

vi.mock('../lib/ipc', () => ({
  fileIpc: {
    getConfig: vi.fn(),
    setInferenceMode: vi.fn(),
    ollamaStatus: vi.fn(),
    updateConfig: vi.fn(),
    signCloudConsent: vi.fn(),
    revokeCloudConsent: vi.fn(),
    getApiKeyStatus: vi.fn(),
    setApiKey: vi.fn(),
    deleteApiKey: vi.fn(),
    importModelPackage: vi.fn(),
  },
}));

import { fileIpc } from '../lib/ipc';
import type { AppConfig } from '../types/ipc';
import { useSettingsStore } from './settingsStore';
import { baseConfig, ollamaOk, resetSettings } from './settingsTestFixtures';

// T1 离线模型包导入：独立 describe 而不是并入主文件——describe 块天然收纳大量用例，
// 按「同一功能一组」组织，避免单个 describe 触函数行数阈值（测试 120 行）。
describe('settingsStore · 离线模型包导入', () => {
  beforeEach(resetSettings);

  it('成功：写入结果、强制刷新探测并复位进行中标志', async () => {
    (fileIpc.importModelPackage as Mock).mockResolvedValue({
      status: 'ok',
      data: { imported: ['bge-large-zh-v1.5'], skipped: [] },
    });
    (fileIpc.ollamaStatus as Mock).mockResolvedValue({ status: 'ok', data: ollamaOk });

    await useSettingsStore.getState().importModelPackage('/tmp/models.zip');

    const s = useSettingsStore.getState();
    expect(fileIpc.importModelPackage).toHaveBeenCalledWith('/tmp/models.zip');
    expect(s.importResult).toEqual({ imported: ['bge-large-zh-v1.5'], skipped: [] });
    expect(s.importError).toBeNull();
    expect(s.importingPackage).toBe(false);
    // 导入可能让模型刚转为就绪：绕过节流重探，让「已就绪 / 未下载」徽标立刻更新
    expect(fileIpc.ollamaStatus).toHaveBeenCalled();
  });

  it('失败：错误落专用字段，不污染通用 error', async () => {
    (fileIpc.importModelPackage as Mock).mockResolvedValue({
      status: 'error',
      error: 'EMB-V-001: 包内未找到模型目录',
    });

    await useSettingsStore.getState().importModelPackage('/tmp/bad.zip');

    const s = useSettingsStore.getState();
    // 通用 error 归 Ollama 探测所有，复用会让「导入失败」被一次后台探测擦掉
    expect(s.importError).toContain('EMB-V-001');
    expect(s.error).toBeNull();
    expect(s.importingPackage).toBe(false);
  });

  it('异常：也复位进行中标志（按钮不会卡在导入中）', async () => {
    (fileIpc.importModelPackage as Mock).mockRejectedValue(new Error('sidecar down'));

    await useSettingsStore.getState().importModelPackage('/tmp/x.zip');

    const s = useSettingsStore.getState();
    expect(s.importError).toBe('sidecar down');
    expect(s.importingPackage).toBe(false);
  });
});

// V019 本地生成后端配置：同样独立成 describe。
describe('settingsStore · 本地生成后端配置', () => {
  beforeEach(resetSettings);

  it('loadConfig 读入后端与 GGUF 标识', async () => {
    (fileIpc.getConfig as Mock).mockResolvedValue({
      status: 'ok',
      data: { ...baseConfig, local_llm_backend: 'builtin', local_llm_model: 'qwen2.5-3b-instruct' },
    });

    await useSettingsStore.getState().loadConfig();

    const s = useSettingsStore.getState();
    expect(s.localLlmBackend).toBe('builtin');
    expect(s.localLlmModel).toBe('qwen2.5-3b-instruct');
  });

  it('loadConfig 缺字段（后端 serde default 的可选字段）→ 回落到 ollama', async () => {
    (fileIpc.getConfig as Mock).mockResolvedValue({ status: 'ok', data: baseConfig });

    await useSettingsStore.getState().loadConfig();

    expect(useSettingsStore.getState().localLlmBackend).toBe('ollama');
  });

  it('updateConfig 不得把后端选择重置回默认值（合并必须回填当前值）', async () => {
    // 同类回归：mergeAppConfig 若漏掉新字段，任何无关的配置更新（如改文件大小）
    // 都会把用户选的 builtin 静默写回 ollama，表现为「重启后后端被打回原样」。
    useSettingsStore.setState({ localLlmBackend: 'builtin' });
    (fileIpc.updateConfig as Mock).mockResolvedValue({
      status: 'ok',
      data: { ...baseConfig, max_file_size_mb: 300, local_llm_backend: 'builtin' },
    });

    await useSettingsStore.getState().updateConfig({ max_file_size_mb: 300 });

    const sent = (fileIpc.updateConfig as Mock).mock.calls[0][0] as AppConfig;
    expect(sent.local_llm_backend).toBe('builtin');
    expect(sent.local_llm_model).toBe('qwen2.5-3b-instruct');
    expect(useSettingsStore.getState().localLlmBackend).toBe('builtin');
  });
});
