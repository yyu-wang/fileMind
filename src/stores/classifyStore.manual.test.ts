// classifyStore 单元测试：手动指定分类（单项 assignCategory 与批量 assignCategories）。

import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('../lib/ipc', () => ({
  fileIpc: {
    classifyPreview: vi.fn(),
    executeOperations: vi.fn(),
    updateFileCategory: vi.fn(),
    undoBatch: vi.fn(),
    listAllFiles: vi.fn(),
    getFileStats: vi.fn(),
    scanDirectory: vi.fn(),
    listCategories: vi.fn(),
  },
}));

import { fileIpc } from '../lib/ipc';
import { useClassifyStore } from './classifyStore';
import { useFileStore } from './fileStore';
import {
  getCategory,
  makeCategory,
  makeItem,
  makePreview,
  OUT_ROOT,
  resetClassifyTestState,
  SCAN_ROOT,
} from './classifyStoreFixtures';

beforeEach(() => {
  resetClassifyTestState();
});

describe('assignCategory', () => {
  async function setupWithPending() {
    const preview = makePreview([
      makeItem('ok'),
      makeItem('pend', {
        category_name: null,
        rule_source: 'needs_review',
        target_path: `${SCAN_ROOT}/pend.png`,
      }),
    ]);
    vi.mocked(fileIpc.classifyPreview).mockResolvedValue({ status: 'ok', data: preview });
    await useClassifyStore.getState().generatePreview(['ok', 'pend']);
    // 直接注入分类缓存（不依赖 loadCategories 异步时序）
    useClassifyStore.setState({
      categories: [makeCategory('c1', '图片', '图片'), makeCategory('c2', '财务', '财务')],
    });
    return preview;
  }

  it('FE-M1: 已分类项拒绝覆盖（分类/stats/pendingIds 全不变）', async () => {
    const preview = await setupWithPending();
    // 先正常分类一次
    useClassifyStore.getState().assignCategory('pend', getCategory('财务'));
    const afterFirst = useClassifyStore.getState().preview;

    // 已分类项再次调用：整体状态保持第一次后的快照
    const category = getCategory('图片');
    const dirOk = useFileStore.getState().scanPath;
    expect(dirOk).toBeTruthy();
    useClassifyStore.getState().assignCategory('pend', category);

    const state = useClassifyStore.getState();
    expect(state.preview).toEqual(afterFirst);
    expect(state.preview?.stats.categorized).toBe(preview.stats.categorized + 1);
    expect(state.pendingIds).toEqual([]);
    const item = state.preview?.items.find((i) => i.file_id === 'pend');
    expect(item?.category_name).toBe('财务');
  });

  it('FE-M2: Error 项手动指定分类后 status 重算为 Ok（可执行）', async () => {
    // 源文件不存在/权限不足导致预览 status='Error' 的项，手动指定即用户授权
    // 执行——不重算则 execute 过滤 (status==='Ok') 永远排除它，静默不执行
    const preview = makePreview([
      makeItem('err', {
        category_name: null,
        rule_source: 'pending',
        status: 'Error',
        conflict_type: null,
      }),
    ]);
    vi.mocked(fileIpc.classifyPreview).mockResolvedValue({ status: 'ok', data: preview });
    await useClassifyStore.getState().generatePreview(['err']);
    useClassifyStore.setState({
      categories: [makeCategory('c1', '文档', '文档')],
    });

    useClassifyStore.getState().assignCategory('err', getCategory('文档'));

    const item = useClassifyStore.getState().preview?.items.find((i) => i.file_id === 'err');
    expect(item?.category_name).toBe('文档');
    expect(item?.rule_source).toBe('manual');
    expect(item?.status).toBe('Ok');
    expect(item?.conflict_type).toBeNull();
  });

  it('FE-M2: Conflict 项手动指定分类后 status 重算为 Ok（目标已变，旧冲突过时）', async () => {
    const preview = makePreview([
      makeItem('conf', {
        category_name: null,
        rule_source: 'pending',
        status: 'Conflict',
        conflict_type: 'SameName',
      }),
    ]);
    vi.mocked(fileIpc.classifyPreview).mockResolvedValue({ status: 'ok', data: preview });
    await useClassifyStore.getState().generatePreview(['conf']);
    useClassifyStore.setState({
      categories: [makeCategory('c1', '图片', '图片')],
    });

    useClassifyStore.getState().assignCategory('conf', getCategory('图片'));

    const item = useClassifyStore.getState().preview?.items.find((i) => i.file_id === 'conf');
    expect(item?.status).toBe('Ok');
    expect(item?.conflict_type).toBeNull();
    // 目标路径随手动指定更新
    expect(item?.target_path).toContain('图片');
  });

  it('assigns category: updates item, stats and pendingIds', async () => {
    const before = await setupWithPending();

    useClassifyStore.getState().assignCategory('pend', getCategory('财务'));

    const state = useClassifyStore.getState();
    expect(state.pendingIds).toEqual([]);
    expect(state.error).toBeNull();
    expect(state.preview?.stats.categorized).toBe(before.stats.categorized + 1);
    expect(state.preview?.stats.pending).toBe(0);

    const item = state.preview?.items.find((i) => i.file_id === 'pend');
    expect(item?.category_name).toBe('财务');
    expect(item?.rule_source).toBe('manual');
    expect(item?.target_path).toBe(`${OUT_ROOT}/财务/pend.png`);
  });

  it('rejects unsafe target_dir', async () => {
    await setupWithPending();
    // 用 target_dir 含路径穿越的分类
    vi.mocked(fileIpc.listCategories).mockResolvedValueOnce({
      status: 'ok',
      data: [makeCategory('bad', '越权', '../secret')],
    });
    await useClassifyStore.getState().loadCategories();
    useClassifyStore.setState({ categories: [makeCategory('bad', '越权', '../secret')] });

    useClassifyStore.getState().assignCategory('pend', getCategory('越权'));

    const state = useClassifyStore.getState();
    expect(state.pendingIds).toEqual(['pend']);
    expect(state.error).toContain('目标目录');
    expect(state.preview?.items.find((i) => i.file_id === 'pend')?.category_name).toBeNull();
  });

  it('is no-op when preview is missing', () => {
    useClassifyStore.setState({ preview: null });
    useClassifyStore.getState().assignCategory('pend', makeCategory('c1', '财务', '财务'));
    expect(useClassifyStore.getState().preview).toBeNull();
  });
});

describe('assignCategories', () => {
  async function setupWithMultiPending() {
    const preview = makePreview([
      makeItem('ok'),
      makeItem('p1', { category_name: null, rule_source: 'needs_review' }),
      makeItem('p2', { category_name: null, rule_source: 'pending' }),
      makeItem('p3', { category_name: null, rule_source: 'pending' }),
    ]);
    vi.mocked(fileIpc.classifyPreview).mockResolvedValue({ status: 'ok', data: preview });
    await useClassifyStore.getState().generatePreview(['ok', 'p1', 'p2', 'p3']);
    // 直接注入分类缓存（不依赖 loadCategories 异步时序）
    useClassifyStore.setState({
      categories: [makeCategory('c1', '图片', '图片'), makeCategory('c2', '财务', '财务')],
    });
    return preview;
  }

  it('assigns a category to multiple files in one call', async () => {
    const before = await setupWithMultiPending();

    useClassifyStore.getState().assignCategories(['p1', 'p2'], getCategory('财务'));

    const state = useClassifyStore.getState();
    // 勾选 2 个：categorized +2、pending -2、pendingIds 移除这两个
    expect(state.preview?.stats.categorized).toBe(before.stats.categorized + 2);
    expect(state.preview?.stats.pending).toBe(1);
    expect(state.pendingIds).toEqual(['p3']);
    expect(state.error).toBeNull();

    const p1 = state.preview?.items.find((i) => i.file_id === 'p1');
    expect(p1?.category_name).toBe('财务');
    expect(p1?.rule_source).toBe('manual');
    expect(p1?.target_path).toBe(`${OUT_ROOT}/财务/p1.png`);
  });

  it('skips already-categorized ids and is no-op when none assigned', async () => {
    await setupWithMultiPending();
    const before = useClassifyStore.getState().preview?.stats.categorized;

    // ok 已分类 → 批量时跳过；无有效项时不应产生状态变化
    useClassifyStore.getState().assignCategories(['ok'], getCategory('财务'));

    const state = useClassifyStore.getState();
    expect(state.preview?.stats.categorized).toBe(before);
    expect(state.pendingIds).toHaveLength(3);
  });

  it('rejects unsafe target_dir without touching preview', async () => {
    await setupWithMultiPending();
    useClassifyStore.setState({
      categories: [makeCategory('bad', '越权', '../secret')],
    });
    const before = useClassifyStore.getState().preview;

    useClassifyStore.getState().assignCategories(['p1', 'p2'], getCategory('越权'));

    const state = useClassifyStore.getState();
    expect(state.error).toContain('目标目录');
    expect(state.preview).toEqual(before);
    expect(state.pendingIds).toHaveLength(3);
  });
});
