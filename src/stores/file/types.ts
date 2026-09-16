// 文件 store 的公共类型与依赖面。
//
// 独立成文件的原因：动作工厂（list / mutation）与 store 本身都要引用 FileState，
// 放在 store 里会让子模块反向依赖 store（循环导入，同 stores/settings/types.ts）。

import type { FileInfo, FileStats, ScannedDirectory } from '@/types/ipc';

/** 写入状态（zustand 的 set）：补丁与函数式更新两种形式都用得到。 */
export type FileSet = (
  partial: Partial<FileState> | ((state: FileState) => Partial<FileState>),
) => void;

/** 读当前状态（zustand 的 get）。 */
export type FileGet = () => FileState;

export interface FileState {
  /** 当前文件列表 */
  files: FileInfo[];
  /** 总文件数（stats 来源） */
  total: number;
  /** 当前扫描路径 */
  scanPath: string | null;
  /** 是否正在扫描 */
  isScanning: boolean;
  /**
   * 是否正在全量拉取文件列表（刷新 / 页面挂载补拉）。
   *
   * 与 `isScanning` 分开：两者都会翻转 `isScanning` 以复用按钮防抖，但空态文案
   * 需要区分「正在扫描目录」与「正在加载列表」——只靠 `isScanning` 无法分辨，
   * 会出现「扫描目录」按钮明明没被点却提示「正在扫描…」。
   */
  isLoadingList: boolean;
  /** 文件库统计 */
  stats: FileStats | null;
  /** 已选中文件的 id 列表 */
  selectedIds: string[];
  /** 已扫描目录列表（目录级移除用） */
  scannedDirectories: ScannedDirectory[];
  /** 错误信息 */
  error: string | null;

  /** 扫描目录并更新文件列表 */
  scanFiles: (path: string) => Promise<void>;
  /** 从库全量拉取文件列表（刷新用，覆盖 files） */
  loadAllFiles: () => Promise<void>;
  /** 加载文件库统计 */
  loadStats: () => Promise<void>;
  /** 加载已扫描目录列表 */
  loadScannedDirectories: () => Promise<void>;
  /** 移除目录：从索引中软删该目录下所有文件 + 清理向量，不删除磁盘文件 */
  removeDirectory: (path: string) => Promise<number>;
  /** 删除文件：把选中的文件移入系统回收站（成功项从列表移除，失败项保留并置 error） */
  deleteFiles: (ids: string[]) => Promise<{ deleted: number; failed: number }>;
  /** 切换单个文件的选中态 */
  toggleSelect: (id: string) => void;
  /** 批量设置选中（全选/清空用） */
  setSelection: (ids: string[]) => void;
  /** 清空选中 */
  clearSelection: () => void;
  /** 清空文件列表 */
  clearFiles: () => void;
  /** 清除错误 */
  clearError: () => void;
}
