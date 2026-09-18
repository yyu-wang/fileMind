// useCitationPreview 单元测试：五条出口路径（空文件名 / 列表补全失败 / 未找到 /
// 慢路径命中 / 内存命中）、持久 toast 的清理（含旧版遗漏的外层异常路径）、
// 卸载守卫与 StrictMode 回归。

import { beforeEach, describe, expect, it, vi } from 'vitest';
import { StrictMode } from 'react';
import { act, renderHook } from '@testing-library/react';

import { useToastStore } from '@/components/ui/Toast';
import { useFileStore } from '@/stores/fileStore';
import type { FileInfo } from '@/types/ipc';
import type { ChatCitation } from '@/types/models';

import { useCitationPreview } from './useCitationPreview';

const mocks = vi.hoisted(() => ({
  searchByFilename: vi.fn(),
  listAllFiles: vi.fn(),
}));

vi.mock('@/lib/ipc', () => ({
  fileIpc: {
    searchByFilename: mocks.searchByFilename,
    listAllFiles: mocks.listAllFiles,
  },
}));

const citation: ChatCitation = { id: 1, fileName: '设计规范.md', page: 3, text: '片段' };

function file(name: string): FileInfo {
  return {
    id: `id-${name}`,
    path: `/tmp/${name}`,
    file_name: name,
    file_size: 100,
    content_hash: null,
    category: null,
    created_at: '2026-08-01 00:00:00',
    updated_at: '2026-08-01 00:00:00',
  };
}

/** 尚未进入退场动画的 toast（remove 会把 id 改成 removing- 前缀）。 */
function activeToasts(): { id: string; message: string }[] {
  return useToastStore.getState().toasts.filter((t) => !t.id.startsWith('removing-'));
}

function toastMessages(): string[] {
  return activeToasts().map((t) => t.message);
}

const originalShow = useToastStore.getState().show;

/** 用 spy 包住 toast 的 show：hook 在调用时读取 getState，可借此断言中间态提示。 */
function spyOnToastShow() {
  const spy = vi.fn(originalShow);
  useToastStore.setState({ show: spy });
  return spy;
}

beforeEach(() => {
  vi.clearAllMocks();
  useToastStore.setState({ toasts: [], show: originalShow });
  useFileStore.setState({ files: [], total: 0 });
  mocks.listAllFiles.mockResolvedValue({ status: 'ok', data: [], total: 0 });
  mocks.searchByFilename.mockResolvedValue({ status: 'ok', data: [] });
});

describe('useCitationPreview', () => {
  it('文件名为空时提示且不发 IPC', async () => {
    const { result } = renderHook(() => useCitationPreview());

    await act(async () => {
      result.current.openCitation({ ...citation, fileName: '' });
    });

    expect(toastMessages()).toEqual(['引用文件名为空，请检查回答内容格式']);
    expect(mocks.listAllFiles).not.toHaveBeenCalled();
    expect(mocks.searchByFilename).not.toHaveBeenCalled();
    expect(result.current.target).toBeNull();
  });

  it('本地列表命中时直接打开，不打扰用户', async () => {
    const hit = file('设计规范.md');
    useFileStore.setState({ files: [hit], total: 1 });
    const showSpy = spyOnToastShow();
    const { result } = renderHook(() => useCitationPreview());

    await act(async () => {
      result.current.openCitation(citation);
    });

    expect(result.current.target).toEqual({ file: hit, initialPage: 3 });
    expect(result.current.mountKey).toBe(1);
    expect(showSpy).not.toHaveBeenCalled();
    expect(mocks.searchByFilename).not.toHaveBeenCalled();
  });

  it('本地列表为空时先补全列表，完成后清掉持久提示', async () => {
    const hit = file('设计规范.md');
    useFileStore.setState({
      files: [],
      loadAllFiles: vi.fn(async () => {
        useFileStore.setState({ files: [hit], total: 1 });
      }),
    });
    const showSpy = spyOnToastShow();
    const { result } = renderHook(() => useCitationPreview());

    await act(async () => {
      result.current.openCitation(citation);
    });

    expect(result.current.target?.file).toBe(hit);
    expect(showSpy.mock.calls.map((call) => call[0].message)).toEqual(['正在定位「设计规范.md」…']);
    expect(toastMessages()).toEqual([]);
  });

  it('列表补全失败时提示，且不打开抽屉', async () => {
    useFileStore.setState({
      files: [],
      loadAllFiles: vi.fn().mockRejectedValue(new Error('ipc down')),
    });
    const { result } = renderHook(() => useCitationPreview());

    await act(async () => {
      result.current.openCitation(citation);
    });

    expect(result.current.target).toBeNull();
    expect(toastMessages()).toEqual(['文件列表加载失败，稍后重试']);
  });

  it('内存与 IPC 都未命中时给出扫描引导', async () => {
    useFileStore.setState({ files: [file('别的.md')], total: 1 });
    const { result } = renderHook(() => useCitationPreview());

    await act(async () => {
      result.current.openCitation(citation);
    });

    expect(result.current.target).toBeNull();
    expect(toastMessages()).toEqual(['未找到引用文件「设计规范.md」，请先在文件管理中扫描该目录']);
  });

  it('IPC 兜底命中时补显示加载提示，完成后一并清掉', async () => {
    const hit = file('设计规范.md');
    useFileStore.setState({ files: [file('别的.md')], total: 1 });
    mocks.searchByFilename.mockResolvedValue({ status: 'ok', data: [hit] });
    const showSpy = spyOnToastShow();
    const { result } = renderHook(() => useCitationPreview());

    await act(async () => {
      result.current.openCitation(citation);
    });

    expect(result.current.target?.file).toBe(hit);
    expect(showSpy.mock.calls.map((call) => call[0].message)).toEqual([
      '正在加载「设计规范.md」预览…',
    ]);
    expect(toastMessages()).toEqual([]);
  });

  // 回归：旧版把清理写在四条出口路径上，外层 catch 漏了清理，而 loading toast
  // 的 duration 为 0（不自动关闭），异常时页面上会永久残留「正在定位…」。
  it('匹配过程抛错时提示失败，且不残留持久提示', async () => {
    useFileStore.setState({
      files: [],
      loadAllFiles: vi.fn(async () => {
        useFileStore.setState({ files: [file('别的.md')], total: 1 });
      }),
    });
    mocks.searchByFilename.mockRejectedValue(new Error('boom'));
    const { result } = renderHook(() => useCitationPreview());

    await act(async () => {
      result.current.openCitation(citation);
    });

    expect(result.current.target).toBeNull();
    expect(toastMessages()).toEqual(['打开预览失败：boom']);
  });

  it('await 期间卸载则不再设置状态', async () => {
    let release: (value: unknown) => void = () => undefined;
    mocks.searchByFilename.mockImplementation(
      () =>
        new Promise((resolve) => {
          release = resolve;
        }),
    );
    useFileStore.setState({ files: [file('别的.md')], total: 1 });
    const { result, unmount } = renderHook(() => useCitationPreview());

    act(() => {
      result.current.openCitation(citation);
    });
    unmount();
    await act(async () => {
      release({ status: 'ok', data: [file('设计规范.md')] });
    });

    expect(result.current.target).toBeNull();
  });

  it('close 清空预览目标', async () => {
    const hit = file('设计规范.md');
    useFileStore.setState({ files: [hit], total: 1 });
    const { result } = renderHook(() => useCitationPreview());

    await act(async () => {
      result.current.openCitation(citation);
    });
    act(() => {
      result.current.close();
    });

    expect(result.current.target).toBeNull();
  });

  // FE-m14 回归：挂载守卫若只在 cleanup 里置 false，StrictMode 双跑 effect 后
  // ref 会永久为 false，点击引用静默 return。
  it('StrictMode 下仍能打开预览', async () => {
    const hit = file('设计规范.md');
    useFileStore.setState({ files: [hit], total: 1 });
    const { result } = renderHook(() => useCitationPreview(), { wrapper: StrictMode });

    await act(async () => {
      result.current.openCitation(citation);
    });

    expect(result.current.target?.file).toBe(hit);
  });
});
