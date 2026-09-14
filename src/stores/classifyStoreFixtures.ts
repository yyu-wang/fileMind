// classifyStore 测试共享夹具：常量、构造器与状态复位。
// 注意：本文件不是测试文件（文件名不含 .test.），不会被 vitest 收集。
//
// `vi.mock('../lib/ipc', ...)` 必须保留在每个测试文件内（vitest 的 vi.mock 按文件
// hoist，且 mock 工厂不能反向 import 本模块——否则会与本模块对 classifyStore/fileStore
// 的依赖构成循环，导致 mock 解析失败）。因此这里只做常量、构造器与复位逻辑的复用。

import { vi } from 'vitest';
import { fileIpc } from '../lib/ipc';
import { ClassifyStatus } from '../types/models';
import type { Category, ClassifyPlanItem, ClassifyPreview, ExecuteResponse } from '../types/ipc';
import { useClassifyStore } from './classifyStore';
import { useFileStore } from './fileStore';

export const SCAN_ROOT = '/tmp/root';
/** 分类输出收纳根（与后端 `sibling_output_root` 同口径：`<扫描根名>_已分类`）。 */
export const OUT_ROOT = `${SCAN_ROOT}_已分类`;

/** T6.12 测试用分类构造器。 */
export function makeCategory(id: string, name: string, targetDir: string): Category {
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
export function getCategory(name: string): Category {
  const cat = useClassifyStore.getState().categories.find((c) => c.name === name);
  if (!cat) throw new Error(`测试前置：分类「${name}」不存在`);
  return cat;
}

export function makeItem(id: string, overrides: Partial<ClassifyPlanItem> = {}): ClassifyPlanItem {
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

export function makePreview(items: ClassifyPlanItem[]): ClassifyPreview {
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

export function okExecute(batchId: string, fileIds: string[]): ExecuteResponse {
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

export function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((r) => {
    resolve = r;
  });
  return { promise, resolve };
}

/** 复位分类/文件 store 状态与各 IPC 默认实现（原测试文件顶层 beforeEach 逻辑）。 */
export function resetClassifyTestState(): void {
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
}
