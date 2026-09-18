// 文件页的渲染数据（订阅 store + 派生），页面按「渲染用什么」订阅这一处。
//
// 与 useFilesPageActions 的分工：本 Hook 只读（含筛选/排序这类页面级派生），
// 动作与本地交互状态在那边；两者互不依赖，页面分别交给头部、工具栏与表格。

import { useFilesPageFilters, type FilesPageFilters } from '@/hooks/useFilesPageFilters';
import { resolveFilesEmptyCopy, type FilesEmptyCopy } from '@/lib/filesPageEmpty';
import { useFileStore } from '@/stores/fileStore';

/** `useFilesPageData` 的返回值。 */
export interface FilesPageData {
  /** 当前扫描根路径（null 时不渲染副标题） */
  scanPath: string | null;
  /** 扫描 / 列表加载中（共用 isScanning 防抖） */
  isScanning: boolean;
  /** 页面级错误提示 */
  error: string | null;
  /** 选中文件 id（表格选中/全选、工具栏计数与删除目标） */
  selectedIds: string[];
  /** 列表为空时的空态文案（非空时 null，直接渲染表格） */
  emptyCopy: FilesEmptyCopy | null;
  /** 搜索/筛选/排序状态与可见列表 */
  filters: FilesPageFilters;
}

/** 订阅文件页渲染所需的数据（含筛选派生与空态文案）。 */
export function useFilesPageData(): FilesPageData {
  const files = useFileStore((s) => s.files);
  const totalFiles = useFileStore((s) => s.total);
  const scanPath = useFileStore((s) => s.scanPath);
  const isScanning = useFileStore((s) => s.isScanning);
  // 区分「扫描目录」与「加载列表」——两者共用 isScanning 防抖，空态文案需要分辨
  const isLoadingList = useFileStore((s) => s.isLoadingList);
  const selectedIds = useFileStore((s) => s.selectedIds);
  const error = useFileStore((s) => s.error);
  const filters = useFilesPageFilters(files);

  const emptyCopy =
    files.length === 0
      ? resolveFilesEmptyCopy({
          isScanning,
          isLoadingList,
          hasScanPath: scanPath !== null,
          totalFiles,
        })
      : null;

  return { scanPath, isScanning, error, selectedIds, emptyCopy, filters };
}
