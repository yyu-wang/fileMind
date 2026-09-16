// 文件页头部：标题 + 扫描路径副标题 + 刷新/扫描目录按钮。

interface FilesPageHeaderProps {
  /** 当前扫描根路径（null 时不渲染副标题） */
  scanPath: string | null;
  /** 扫描 / 列表加载中（共用 isScanning 防抖） */
  isScanning: boolean;
  /** 刷新：重新取回全量文件列表 */
  onRefresh: () => void;
  /** 扫描目录 */
  onScan: () => void;
}

export function FilesPageHeader({ scanPath, isScanning, onRefresh, onScan }: FilesPageHeaderProps) {
  return (
    <header className="main-header">
      <h1>文件管理</h1>
      {scanPath && (
        <span className="subtitle" title={scanPath}>
          {scanPath}
        </span>
      )}
      <div className="header-actions">
        {/* FE-C4：刷新也进 isScanning 态（store），狂点被防抖 */}
        <button
          type="button"
          className="btn btn--ghost btn--sm"
          onClick={onRefresh}
          disabled={isScanning}
        >
          刷新
        </button>
        <button
          type="button"
          className="btn btn--primary btn--sm"
          data-testid="files-scan"
          onClick={onScan}
          disabled={isScanning}
        >
          {isScanning ? '扫描中…' : '扫描目录'}
        </button>
      </div>
    </header>
  );
}
