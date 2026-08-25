// fileStore 单元测试：扫描/拉取/统计/选中态的 IPC 驱动与错误分支。

import { beforeEach, describe, expect, it, vi, type Mock } from 'vitest';

vi.mock('../lib/ipc', () => ({
  fileIpc: {
    scanDirectory: vi.fn(),
    listAllFiles: vi.fn(),
    getFileStats: vi.fn(),
  },
}));

import { fileIpc } from '../lib/ipc';
import type { FileInfo, FileStats } from '../types/ipc';
import { useFileStore } from './fileStore';

function fileInfo(id: string, name: string): FileInfo {
  return {
    id,
    path: `/tmp/${name}`,
    file_name: name,
    file_size: 100,
    content_hash: 'abc',
    category: null,
    created_at: '2026-08-23T10:00:00Z',
    updated_at: '2026-08-23T10:00:00Z',
  };
}

const stats: FileStats = {
  total_files: 10,
  categorized_files: 4,
  uncategorized_files: 6,
  duplicate_groups: 1,
  total_size_bytes: 1024,
};

function freshState(): void {
  useFileStore.setState({
    files: [],
    total: 0,
    scanPath: null,
    isScanning: false,
    stats: null,
    selectedIds: [],
    error: null,
  });
}

describe('fileStore', () => {
  beforeEach(() => {
    freshState();
    vi.clearAllMocks();
    // 默认让统计接口可用：多数用例依赖扫描/拉取成功后自动刷新统计
    (fileIpc.getFileStats as Mock).mockResolvedValue({ status: 'ok', data: stats });
  });

  it('FE-C4: 慢扫描 A + 快扫描 B 并发，A 晚回被丢弃（scanPath 与 files 一致）', async () => {
    const filesA = [fileInfo('1', 'a.pdf')];
    const filesB = [fileInfo('2', 'b.pdf')];
    let resolveA!: (v: { status: string; data: FileInfo[] }) => void;
    (fileIpc.scanDirectory as Mock).mockImplementation((path: string) =>
      path === '/a'
        ? new Promise((r) => {
            resolveA = r;
          })
        : Promise.resolve({ status: 'ok', data: filesB }),
    );

    const p1 = useFileStore.getState().scanFiles('/a');
    const p2 = useFileStore.getState().scanFiles('/b');
    await p2;
    // B 已返回；此时 A 的慢响应才到达
    resolveA({ status: 'ok', data: filesA });
    await p1;

    const s = useFileStore.getState();
    expect(s.files).toEqual(filesB);
    expect(s.scanPath).toBe('/b');
    expect(s.isScanning).toBe(false);
  });

  it('FE-C4: loadAllFiles 成功清空 scanPath（全量列表与旧扫描根解绑）', async () => {
    useFileStore.setState({ scanPath: '/old' });
    (fileIpc.listAllFiles as Mock).mockResolvedValue({
      status: 'ok',
      data: [fileInfo('1', 'x.pdf')],
    });

    await useFileStore.getState().loadAllFiles();

    const s = useFileStore.getState();
    expect(s.scanPath).toBeNull();
    expect(s.files).toHaveLength(1);
    expect(s.isScanning).toBe(false);
  });

  it('FE-C4: loadAllFiles 期间 isScanning=true（刷新按钮可禁用）', async () => {
    let resolve!: (v: { status: string; data: FileInfo[] }) => void;
    (fileIpc.listAllFiles as Mock).mockImplementation(
      () =>
        new Promise((r) => {
          resolve = r;
        }),
    );
    const p = useFileStore.getState().loadAllFiles();
    expect(useFileStore.getState().isScanning).toBe(true);
    resolve({ status: 'ok', data: [] });
    await p;
    expect(useFileStore.getState().isScanning).toBe(false);
  });

  it('scanFiles 成功：更新列表/路径/统计并清空选中', async () => {
    const files = [fileInfo('1', 'a.pdf'), fileInfo('2', 'b.pdf')];
    (fileIpc.scanDirectory as Mock).mockResolvedValue({ status: 'ok', data: files });
    useFileStore.getState().toggleSelect('1'); // 预置选中，扫描后应清空

    await useFileStore.getState().scanFiles('/tmp');

    const s = useFileStore.getState();
    expect(s.isScanning).toBe(false);
    expect(s.files).toEqual(files);
    expect(s.scanPath).toBe('/tmp');
    expect(s.selectedIds).toEqual([]);
    expect(s.stats).toEqual(stats); // 完成后自动 loadStats
    expect(s.total).toBe(10);
    expect(fileIpc.getFileStats).toHaveBeenCalled();
  });

  it('scanFiles 失败：置错并结束扫描态', async () => {
    (fileIpc.scanDirectory as Mock).mockResolvedValue({ status: 'error', error: 'DIR-E-001' });

    await useFileStore.getState().scanFiles('/tmp');

    const s = useFileStore.getState();
    expect(s.isScanning).toBe(false);
    expect(s.error).toBe('DIR-E-001');
    expect(s.files).toEqual([]);
  });

  it('loadAllFiles 成功：覆盖列表、清空选中并刷新统计', async () => {
    const files = [fileInfo('1', 'a.pdf')];
    (fileIpc.listAllFiles as Mock).mockResolvedValue({ status: 'ok', data: files });
    useFileStore.getState().toggleSelect('1');

    await useFileStore.getState().loadAllFiles();

    const s = useFileStore.getState();
    expect(fileIpc.listAllFiles).toHaveBeenCalledWith(null);
    expect(s.files).toEqual(files);
    expect(s.selectedIds).toEqual([]);
    expect(s.total).toBe(10);
  });

  it('loadAllFiles 失败：仅置错', async () => {
    (fileIpc.listAllFiles as Mock).mockResolvedValue({ status: 'error', error: 'DB-U-001' });

    await useFileStore.getState().loadAllFiles();

    expect(useFileStore.getState().error).toBe('DB-U-001');
  });

  it('loadStats 成功：写入 stats 与 total', async () => {
    await useFileStore.getState().loadStats();

    const s = useFileStore.getState();
    expect(s.stats).toEqual(stats);
    expect(s.total).toBe(10);
  });

  it('loadStats 失败：置错', async () => {
    (fileIpc.getFileStats as Mock).mockResolvedValue({ status: 'error', error: 'DB-U-001' });

    await useFileStore.getState().loadStats();

    expect(useFileStore.getState().error).toBe('DB-U-001');
  });

  it('toggleSelect：重复点击增删选中', () => {
    useFileStore.getState().toggleSelect('1');
    expect(useFileStore.getState().selectedIds).toEqual(['1']);
    useFileStore.getState().toggleSelect('1');
    expect(useFileStore.getState().selectedIds).toEqual([]);
  });

  it('setSelection / clearSelection：批量设置与清空', () => {
    useFileStore.getState().setSelection(['1', '2']);
    expect(useFileStore.getState().selectedIds).toEqual(['1', '2']);

    useFileStore.getState().clearSelection();
    expect(useFileStore.getState().selectedIds).toEqual([]);
  });

  it('clearFiles：清空列表/路径/选中', () => {
    useFileStore.setState({
      files: [fileInfo('1', 'a.pdf')],
      scanPath: '/tmp',
      selectedIds: ['1'],
    });

    useFileStore.getState().clearFiles();

    const s = useFileStore.getState();
    expect(s.files).toEqual([]);
    expect(s.scanPath).toBeNull();
    expect(s.selectedIds).toEqual([]);
  });

  it('clearError：清除错误', () => {
    useFileStore.setState({ error: 'boom' });

    useFileStore.getState().clearError();

    expect(useFileStore.getState().error).toBeNull();
  });
});
