// 文件页挂载兜底加载（从 FilesPage 的 useEffect 抽出，页面只留一行调用）。
//
// 启动时只取了 stats（main.tsx）与已扫描目录（面板自身 effect），文件列表却要等用户点
// 「刷新」——于是「状态栏 47 文件 + 已扫描目录 1 个」会和主区「尚未扫描目录」同时出现。
// 这里在确有已索引文件时补拉一次全量列表（list_all_files 明确为虚拟滚动表「一次取回全部」
// 而设计，表侧也是虚拟滚动）。
//
// 门闩防重入：loadAllFiles 会翻转 isScanning，不锁会让 effect 反复触发
// （ChatPage 同款门闩，真实翻车表现是 IPC 刷屏 + 页面反复重渲染）。

import { useEffect, useRef } from 'react';

import { useFileStore } from '@/stores/fileStore';

export function useFilesBootstrap(): void {
  const isScanning = useFileStore((s) => s.isScanning);
  const fileCount = useFileStore((s) => s.files.length);
  const totalFiles = useFileStore((s) => s.total);
  const loadAllFiles = useFileStore((s) => s.loadAllFiles);
  const bootstrappedRef = useRef(false);

  useEffect(() => {
    if (bootstrappedRef.current || isScanning || fileCount > 0) return;
    if (totalFiles === 0) return;
    bootstrappedRef.current = true;
    void loadAllFiles().catch(() => {
      // 兜底：loadAllFiles 内部已吞掉 IPC 失败（写 store.error 横幅）并通过「刷新」
      // 提供重试入口；这里只防意外抛出把门闩锁死
      bootstrappedRef.current = false;
    });
  }, [totalFiles, isScanning, fileCount, loadAllFiles]);
}
