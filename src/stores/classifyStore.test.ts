// classifyStore 单元测试：预览生成、分块执行、待确认/冲突排除、暂停/取消、撤销。

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
import { ClassifyStatus } from '../types/models';
import type { Category, ClassifyPlanItem, ClassifyPreview, ExecuteResponse } from '../types/ipc';
import { useClassifyStore } from './classifyStore';
import { useFileStore } from './fileStore';

const SCAN_ROOT = '/tmp/root';
/** 分类输出收纳根（与后端 `sibling_output_root` 同口径：`<扫描根名>_已分类`）。 */
const OUT_ROOT = `${SCAN_ROOT}_已分类`;

/** T6.12 测试用分类构造器。 */
function makeCategory(id: string, name: string, targetDir: string): Category {
  return {
    id,
    name,
    parent_id: null,
    icon: null,
    color: null,
    sort_order: 0,
    is_builtin: false,
    target_dir: targetDir,
    created_at: '2026-01-01 00:00:00',
    updated_at: '2026-01-01 00:00:00',
  };
}

/** 从 store 缓存按名取分类，取不到直接抛错（测试前置断言）。 */
function getCategory(name: string): Category {
  const cat = useClassifyStore.getState().categories.find((c) => c.name === name);
  if (!cat) throw new Error(`测试前置：分类「${name}」不存在`);
  return cat;
}

function makeItem(id: string, overrides: Partial<ClassifyPlanItem> = {}): ClassifyPlanItem {
  return {
    file_id: id,
    file_name: `${id}.png`,
    original_path: `${SCAN_ROOT}/${id}.png`,
    target_path: `${OUT_ROOT}/图片/${id}.png`,
    category_name: '图片',
    rule_source: 'heuristic',
    status: 'Ok',
    conflict_type: null,
    ...overrides,
  };
}

function makePreview(items: ClassifyPlanItem[]): ClassifyPreview {
  const categorized = items.filter((i) => i.category_name != null).length;
  return {
    batch_id: 'batch-preview',
    output_root: OUT_ROOT,
    items,
    stats: {
      total: items.length,
      categorized,
      pending: items.length - categorized,
      by_rule: 0,
      by_heuristic: categorized,
    },
  };
}

function okExecute(batchId: string, fileIds: string[]): ExecuteResponse {
  return {
    batch_id: batchId,
    results: fileIds.map((fid) => ({
      file_id: fid,
      operation: 'Move',
      source_path: `${SCAN_ROOT}/${fid}.png`,
      target_path: `${OUT_ROOT}/图片/${fid}.png`,
      success: true,
      error: null,
      prev_hash: null,
      current_hash: null,
    })),
    summary: { total: fileIds.length, success: fileIds.length, failed: 0, skipped: 0 },
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((r) => {
    resolve = r;
  });
  return { promise, resolve };
}

beforeEach(() => {
  useClassifyStore.setState({
    status: ClassifyStatus.Idle,
    preview: null,
    pendingIds: [],
    categories: [],
    progress: { done: 0, total: 0 },
    execSummary: null,
    lastBatchId: null,
    error: null,
  });
  useFileStore.setState({ scanPath: SCAN_ROOT, files: [], selectedIds: [] });
  vi.clearAllMocks();
  vi.mocked(fileIpc.listAllFiles).mockResolvedValue({ status: 'ok', data: [] });
  // FE-C6 后 execute 会读取打标返回值的 status：给默认成功实现，
  // 需要模拟失败的用例再用 mockResolvedValueOnce 覆盖。
  vi.mocked(fileIpc.updateFileCategory).mockResolvedValue({ status: 'ok', data: null });
  vi.mocked(fileIpc.listCategories).mockResolvedValue({
    status: 'ok',
    data: [makeCategory('c1', '图片', '图片'), makeCategory('c2', '财务', '财务')],
  });
  vi.mocked(fileIpc.getFileStats).mockResolvedValue({
    status: 'ok',
    data: {
      total_files: 0,
      categorized_files: 0,
      uncategorized_files: 0,
      duplicate_groups: 0,
      total_size_bytes: 0,
    },
  });
});

describe('generatePreview（FE-M9 守卫）', () => {
  it('FE-M9: Previewing 中重入直接返回（classifyPreview 仅一次）', async () => {
    // 手写 deferred：类型与 mock 返回联合对齐，避免 unknown 不兼容
    let resolvePreview!: (v: Awaited<ReturnType<typeof fileIpc.classifyPreview>>) => void;
    vi.mocked(fileIpc.classifyPreview).mockImplementation(
      () =>
        new Promise((r) => {
          resolvePreview = r as typeof resolvePreview;
        }),
    );

    const p1 = useClassifyStore.getState().generatePreview(['a']);
    const p2 = useClassifyStore.getState().generatePreview(['a']);
    resolvePreview({ status: 'ok', data: makePreview([makeItem('a')]) });
    await Promise.all([p1, p2]);

    expect(fileIpc.classifyPreview).toHaveBeenCalledTimes(1);
    expect(useClassifyStore.getState().status).toBe(ClassifyStatus.Idle);
  });

  it('FE-M9: 过期响应丢弃——reset 后到达的慢响应不覆盖状态', async () => {
    let resolvePreview!: (v: Awaited<ReturnType<typeof fileIpc.classifyPreview>>) => void;
    vi.mocked(fileIpc.classifyPreview).mockImplementation(
      () =>
        new Promise((r) => {
          resolvePreview = r as typeof resolvePreview;
        }),
    );

    const p = useClassifyStore.getState().generatePreview(['a']);
    // 用户在响应到达前取消（reset 作废 seq），随后慢响应才到达
    useClassifyStore.getState().reset();
    resolvePreview({ status: 'ok', data: makePreview([makeItem('a')]) });
    await p;

    // reset 后状态保持 Idle/preview=null，不被慢响应覆盖
    const s = useClassifyStore.getState();
    expect(s.status).toBe(ClassifyStatus.Idle);
    expect(s.preview).toBeNull();
  });
});

describe('generatePreview', () => {
  it('calls classifyPreview with scanPath from fileStore and stores pending ids', async () => {
    const preview = makePreview([
      makeItem('a'),
      makeItem('p', { category_name: null, rule_source: 'pending' }),
    ]);
    vi.mocked(fileIpc.classifyPreview).mockResolvedValue({ status: 'ok', data: preview });

    await useClassifyStore.getState().generatePreview(['a', 'p']);

    expect(fileIpc.classifyPreview).toHaveBeenCalledWith(['a', 'p'], SCAN_ROOT);
    expect(useClassifyStore.getState().status).toBe(ClassifyStatus.Idle);
    expect(useClassifyStore.getState().preview).toEqual(preview);
    expect(useClassifyStore.getState().pendingIds).toEqual(['p']);
  });

  it('sets error without calling IPC when scanPath is missing', async () => {
    useFileStore.setState({ scanPath: null });
    await useClassifyStore.getState().generatePreview(['a']);

    expect(fileIpc.classifyPreview).not.toHaveBeenCalled();
    expect(useClassifyStore.getState().error).toBe('请先在文件页选择要整理的目录');
  });

  it('sets error and stays Idle when preview fails', async () => {
    vi.mocked(fileIpc.classifyPreview).mockResolvedValue({ status: 'error', error: '扫描根无效' });
    await useClassifyStore.getState().generatePreview(['a']);

    expect(useClassifyStore.getState().status).toBe(ClassifyStatus.Idle);
    expect(useClassifyStore.getState().error).toBe('扫描根无效');
  });
});

describe('execute', () => {
  it('splits plan into chunks and updates category for each success', async () => {
    const ids = Array.from({ length: 55 }, (_, n) => `f${n}`);
    const items = ids.map((id) => makeItem(id));
    vi.mocked(fileIpc.classifyPreview).mockResolvedValue({
      status: 'ok',
      data: makePreview(items),
    });
    await useClassifyStore.getState().generatePreview(ids);

    vi.mocked(fileIpc.executeOperations)
      .mockResolvedValueOnce({ status: 'ok', data: okExecute('b1', ids.slice(0, 50)) })
      .mockResolvedValueOnce({ status: 'ok', data: okExecute('b1', ids.slice(50)) });

    await useClassifyStore.getState().execute();

    expect(fileIpc.executeOperations).toHaveBeenCalledTimes(2);
    const firstPlan = vi.mocked(fileIpc.executeOperations).mock.calls[0][0].plan;
    const secondPlan = vi.mocked(fileIpc.executeOperations).mock.calls[1][0].plan;
    expect(firstPlan).toHaveLength(50);
    expect(secondPlan).toHaveLength(5);
    expect(firstPlan[0].operation).toBe('Move');
    expect(firstPlan[0].new_path).toBe(items[0].target_path);
    expect(fileIpc.updateFileCategory).toHaveBeenCalledTimes(55);
    expect(fileIpc.updateFileCategory).toHaveBeenCalledWith('f0', '图片');

    const state = useClassifyStore.getState();
    expect(state.status).toBe(ClassifyStatus.Done);
    expect(state.progress).toEqual({ done: 55, total: 55 });
    expect(state.execSummary).toEqual({ success: 55, failed: 0, pending: 0, total: 55 });
    expect(state.lastBatchId).toBe('b1');
  });

  it('copy mode sends Copy operation and still labels original files', async () => {
    const items = [makeItem('a'), makeItem('b')];
    vi.mocked(fileIpc.classifyPreview).mockResolvedValue({
      status: 'ok',
      data: makePreview(items),
    });
    await useClassifyStore.getState().generatePreview(['a', 'b']);

    vi.mocked(fileIpc.executeOperations).mockResolvedValue({
      status: 'ok',
      data: okExecute('b1', ['a', 'b']),
    });

    await useClassifyStore.getState().execute(false, 'copy');

    const plan = vi.mocked(fileIpc.executeOperations).mock.calls[0][0].plan;
    expect(plan).toHaveLength(2);
    expect(plan[0].operation).toBe('Copy');
    expect(plan[0].new_path).toBe(items[0].target_path);
    // 复制模式同样给原文件打分类标签（软排除，避免重复选中）
    expect(fileIpc.updateFileCategory).toHaveBeenCalledTimes(2);
    expect(fileIpc.updateFileCategory).toHaveBeenCalledWith('a', '图片');

    const state = useClassifyStore.getState();
    expect(state.status).toBe(ClassifyStatus.Done);
    expect(state.execSummary).toEqual({ success: 2, failed: 0, pending: 0, total: 2 });
  });

  it('excludes pending and conflict items from execution plan', async () => {
    const items = [
      makeItem('ok'),
      makeItem('pend', { category_name: null, rule_source: 'pending' }),
      makeItem('conf', { status: 'Conflict', conflict_type: 'SameName' }),
    ];
    vi.mocked(fileIpc.classifyPreview).mockResolvedValue({
      status: 'ok',
      data: makePreview(items),
    });
    await useClassifyStore.getState().generatePreview(['ok', 'pend', 'conf']);

    vi.mocked(fileIpc.executeOperations).mockResolvedValue({
      status: 'ok',
      data: okExecute('b1', ['ok']),
    });
    await useClassifyStore.getState().execute();

    const plan = vi.mocked(fileIpc.executeOperations).mock.calls[0][0].plan;
    expect(plan.map((p) => p.file_id)).toEqual(['ok']);
    expect(fileIpc.updateFileCategory).toHaveBeenCalledTimes(1);
    expect(useClassifyStore.getState().execSummary?.pending).toBe(1);
  });

  it('pause and resume toggle status during an in-flight chunk', async () => {
    const items = [makeItem('a')];
    vi.mocked(fileIpc.classifyPreview).mockResolvedValue({
      status: 'ok',
      data: makePreview(items),
    });
    await useClassifyStore.getState().generatePreview(['a']);

    const gate = deferred<{ status: 'ok'; data: ExecuteResponse }>();
    vi.mocked(fileIpc.executeOperations).mockReturnValueOnce(gate.promise);

    const execPromise = useClassifyStore.getState().execute();
    await vi.waitFor(() => expect(fileIpc.executeOperations).toHaveBeenCalled());
    expect(useClassifyStore.getState().status).toBe(ClassifyStatus.Running);

    useClassifyStore.getState().pause();
    expect(useClassifyStore.getState().status).toBe(ClassifyStatus.Paused);

    useClassifyStore.getState().resume();
    expect(useClassifyStore.getState().status).toBe(ClassifyStatus.Running);

    gate.resolve({ status: 'ok', data: okExecute('b1', ['a']) });
    await execPromise;
    expect(useClassifyStore.getState().status).toBe(ClassifyStatus.Done);
  });

  it('cancel stops further chunks but keeps executed results', async () => {
    const ids = Array.from({ length: 55 }, (_, n) => `f${n}`);
    const items = ids.map((id) => makeItem(id));
    vi.mocked(fileIpc.classifyPreview).mockResolvedValue({
      status: 'ok',
      data: makePreview(items),
    });
    await useClassifyStore.getState().generatePreview(ids);

    const gate = deferred<{ status: 'ok'; data: ExecuteResponse }>();
    vi.mocked(fileIpc.executeOperations).mockReturnValueOnce(gate.promise);

    const execPromise = useClassifyStore.getState().execute();
    await vi.waitFor(() => expect(fileIpc.executeOperations).toHaveBeenCalled());

    useClassifyStore.getState().cancel();
    expect(useClassifyStore.getState().status).toBe(ClassifyStatus.Cancelled);

    gate.resolve({ status: 'ok', data: okExecute('b1', ids.slice(0, 50)) });
    await execPromise;

    expect(fileIpc.executeOperations).toHaveBeenCalledTimes(1);
    const state = useClassifyStore.getState();
    expect(state.status).toBe(ClassifyStatus.Cancelled);
    expect(state.execSummary?.success).toBe(50);
    expect(state.progress).toEqual({ done: 50, total: 55 });
  });

  it('resolveConflicts=true includes conflict items and passes the flag', async () => {
    const items = [
      makeItem('ok'),
      makeItem('conf', { status: 'Conflict', conflict_type: 'SameName' }),
    ];
    vi.mocked(fileIpc.classifyPreview).mockResolvedValue({
      status: 'ok',
      data: makePreview(items),
    });
    await useClassifyStore.getState().generatePreview(['ok', 'conf']);

    vi.mocked(fileIpc.executeOperations).mockResolvedValue({
      status: 'ok',
      data: okExecute('b1', ['ok', 'conf']),
    });
    await useClassifyStore.getState().execute(true);

    const req = vi.mocked(fileIpc.executeOperations).mock.calls[0][0];
    expect(req.resolve_conflicts).toBe(true);
    expect(req.plan.map((p) => p.file_id)).toEqual(['ok', 'conf']);
    expect(useClassifyStore.getState().execSummary?.success).toBe(2);
  });

  it('default execute excludes conflict items without resolving', async () => {
    const items = [
      makeItem('ok'),
      makeItem('conf', { status: 'Conflict', conflict_type: 'SameName' }),
    ];
    vi.mocked(fileIpc.classifyPreview).mockResolvedValue({
      status: 'ok',
      data: makePreview(items),
    });
    await useClassifyStore.getState().generatePreview(['ok', 'conf']);

    vi.mocked(fileIpc.executeOperations).mockResolvedValue({
      status: 'ok',
      data: okExecute('b1', ['ok']),
    });
    await useClassifyStore.getState().execute();

    const req = vi.mocked(fileIpc.executeOperations).mock.calls[0][0];
    expect(req.resolve_conflicts).toBe(false);
    expect(req.plan.map((p) => p.file_id)).toEqual(['ok']);
  });

  // FE-C1：执行中/暂停中再次 execute 直接返回，不产生第二次 IPC。
  it('rejects reentrant execute while Running without a second IPC call', async () => {
    const items = [makeItem('a')];
    vi.mocked(fileIpc.classifyPreview).mockResolvedValue({
      status: 'ok',
      data: makePreview(items),
    });
    await useClassifyStore.getState().generatePreview(['a']);

    const gate = deferred<{ status: 'ok'; data: ExecuteResponse }>();
    vi.mocked(fileIpc.executeOperations).mockReturnValueOnce(gate.promise);

    const execPromise = useClassifyStore.getState().execute();
    await vi.waitFor(() => expect(fileIpc.executeOperations).toHaveBeenCalled());
    expect(useClassifyStore.getState().status).toBe(ClassifyStatus.Running);

    await useClassifyStore.getState().execute();
    expect(fileIpc.executeOperations).toHaveBeenCalledTimes(1);

    gate.resolve({ status: 'ok', data: okExecute('b1', ['a']) });
    await execPromise;
  });

  it('rejects reentrant execute while Paused without a second IPC call', async () => {
    const items = [makeItem('a')];
    vi.mocked(fileIpc.classifyPreview).mockResolvedValue({
      status: 'ok',
      data: makePreview(items),
    });
    await useClassifyStore.getState().generatePreview(['a']);

    const gate = deferred<{ status: 'ok'; data: ExecuteResponse }>();
    vi.mocked(fileIpc.executeOperations).mockReturnValueOnce(gate.promise);

    const execPromise = useClassifyStore.getState().execute();
    await vi.waitFor(() => expect(fileIpc.executeOperations).toHaveBeenCalled());
    useClassifyStore.getState().pause();
    expect(useClassifyStore.getState().status).toBe(ClassifyStatus.Paused);

    await useClassifyStore.getState().execute();
    expect(fileIpc.executeOperations).toHaveBeenCalledTimes(1);

    useClassifyStore.getState().resume();
    gate.resolve({ status: 'ok', data: okExecute('b1', ['a']) });
    await execPromise;
  });

  // FE-C5：执行中 reset 后，旧循环收尾不得把状态覆盖回 Cancelled/Done。
  it('reset during execution prevents stale loop from overwriting state', async () => {
    const items = [makeItem('a')];
    vi.mocked(fileIpc.classifyPreview).mockResolvedValue({
      status: 'ok',
      data: makePreview(items),
    });
    await useClassifyStore.getState().generatePreview(['a']);

    const gate = deferred<{ status: 'ok'; data: ExecuteResponse }>();
    vi.mocked(fileIpc.executeOperations).mockReturnValueOnce(gate.promise);

    const execPromise = useClassifyStore.getState().execute();
    await vi.waitFor(() => expect(fileIpc.executeOperations).toHaveBeenCalled());

    useClassifyStore.getState().reset();
    expect(useClassifyStore.getState().status).toBe(ClassifyStatus.Idle);
    expect(useClassifyStore.getState().preview).toBeNull();

    gate.resolve({ status: 'ok', data: okExecute('b1', ['a']) });
    await execPromise;

    // 旧循环恢复后令牌失配：不得写回状态/execSummary/lastBatchId。
    const state = useClassifyStore.getState();
    expect(state.status).toBe(ClassifyStatus.Idle);
    expect(state.execSummary).toBeNull();
    expect(state.lastBatchId).toBeNull();
    expect(state.progress).toEqual({ done: 0, total: 0 });
  });

  // FE-C6：打标失败计入 failed 并提示，后续文件继续执行。
  it('counts label failure into failed and continues remaining items', async () => {
    const items = [makeItem('a'), makeItem('b')];
    vi.mocked(fileIpc.classifyPreview).mockResolvedValue({
      status: 'ok',
      data: makePreview(items),
    });
    await useClassifyStore.getState().generatePreview(['a', 'b']);

    vi.mocked(fileIpc.executeOperations).mockResolvedValue({
      status: 'ok',
      data: okExecute('b1', ['a', 'b']),
    });
    vi.mocked(fileIpc.updateFileCategory)
      .mockResolvedValueOnce({ status: 'error', error: '数据库锁定' })
      .mockResolvedValueOnce({ status: 'ok', data: null });

    await useClassifyStore.getState().execute();

    const state = useClassifyStore.getState();
    expect(state.status).toBe(ClassifyStatus.Done);
    expect(state.execSummary).toEqual({ success: 1, failed: 1, pending: 0, total: 2 });
    expect(state.error).toContain('分类标签写入失败');
    // 失败不中断整体执行：两个文件的打标调用都发生。
    expect(fileIpc.updateFileCategory).toHaveBeenCalledTimes(2);
  });
});

describe('undoLastBatch', () => {
  it('calls undoBatch with the last batch id and resets to Idle', async () => {
    const items = [makeItem('a')];
    vi.mocked(fileIpc.classifyPreview).mockResolvedValue({
      status: 'ok',
      data: makePreview(items),
    });
    await useClassifyStore.getState().generatePreview(['a']);
    vi.mocked(fileIpc.executeOperations).mockResolvedValue({
      status: 'ok',
      data: okExecute('b1', ['a']),
    });
    await useClassifyStore.getState().execute();
    expect(useClassifyStore.getState().lastBatchId).toBe('b1');

    vi.mocked(fileIpc.undoBatch).mockResolvedValue({
      status: 'ok',
      data: { success: true, undone_count: 1, failed_count: 0, task_id: 't1' },
    });
    await useClassifyStore.getState().undoLastBatch();

    expect(fileIpc.undoBatch).toHaveBeenCalledWith('b1');
    expect(useClassifyStore.getState().status).toBe(ClassifyStatus.Idle);
    expect(useClassifyStore.getState().lastBatchId).toBeNull();
  });

  it('sets error when no batch id exists', async () => {
    await useClassifyStore.getState().undoLastBatch();
    expect(fileIpc.undoBatch).not.toHaveBeenCalled();
    expect(useClassifyStore.getState().error).toBe('无最近批次可撤销');
  });
});

describe('reset', () => {
  it('clears preview, progress and summary', async () => {
    const items = [makeItem('a')];
    vi.mocked(fileIpc.classifyPreview).mockResolvedValue({
      status: 'ok',
      data: makePreview(items),
    });
    await useClassifyStore.getState().generatePreview(['a']);

    useClassifyStore.getState().reset();

    const state = useClassifyStore.getState();
    expect(state.status).toBe(ClassifyStatus.Idle);
    expect(state.preview).toBeNull();
    expect(state.pendingIds).toEqual([]);
    expect(state.progress).toEqual({ done: 0, total: 0 });
    expect(state.execSummary).toBeNull();
    expect(state.error).toBeNull();
  });
});

describe('loadCategories', () => {
  it('loads categories once and caches (idempotent)', async () => {
    await useClassifyStore.getState().loadCategories();
    expect(fileIpc.listCategories).toHaveBeenCalledTimes(1);
    expect(useClassifyStore.getState().categories).toHaveLength(2);

    await useClassifyStore.getState().loadCategories();
    expect(fileIpc.listCategories).toHaveBeenCalledTimes(1);
  });
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
