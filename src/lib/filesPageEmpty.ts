// 文件页空态文案（纯函数，可单测）：区分「从未扫描 / 扫描中 / 列表加载中 / 已索引但列表未取回」。
//
// 此前只看 `files.length === 0` 就说「尚未扫描目录」，会出现「状态栏 47 文件 +
// 已扫描目录 1 个」与主区「尚未扫描目录」同时成立的语义矛盾。判定从页面组件抽到
// 本模块后，页面只负责编排（把结果交给空态区域渲染）。

/** 空态文案：主标题 + 兜底副标题（null 表示不显示副标题）。 */
export interface FilesEmptyCopy {
  title: string;
  sub: string | null;
}

/** 空态判定输入（直接取自 store 状态）。 */
export interface FilesEmptyCopyInput {
  /** 是否处于扫描 / 列表加载中（两者共用 isScanning 防抖） */
  isScanning: boolean;
  /** 是否正在加载文件列表（区分「正在加载文件列表…」与「正在扫描…」） */
  isLoadingList: boolean;
  /** 是否已有扫描根路径 */
  hasScanPath: boolean;
  /** 已索引文件总数（stats 来源） */
  totalFiles: number;
}

/** 由 store 状态派生文件页空态文案。 */
export function resolveFilesEmptyCopy({
  isScanning,
  isLoadingList,
  hasScanPath,
  totalFiles,
}: FilesEmptyCopyInput): FilesEmptyCopy {
  if (isScanning) {
    return { title: isLoadingList ? '正在加载文件列表…' : '正在扫描…', sub: null };
  }
  if (hasScanPath) {
    return { title: '当前目录暂无文件', sub: '点击「扫描目录」重新扫描' };
  }
  if (totalFiles > 0) {
    // 兜底出口：正常路径下自动加载即可填满列表，这里只服务「自动加载失败」的情况
    return { title: '文件列表尚未加载', sub: `已索引 ${totalFiles} 个文件，点击「刷新」加载` };
  }
  return { title: '尚未扫描目录', sub: '点击「扫描目录」选择要管理的文件夹' };
}
