// 已扫描目录面板：列出所有已扫描的根目录，支持目录级移除。
//
// 移除语义：仅从 FileMind 索引（SQLite + 向量库）中移除该目录下的所有文件，
// 不删除磁盘上的文件。重新扫描该目录即可恢复。

import { useEffect, useState } from 'react';

import { ConfirmDialog } from '@/components/ui/ConfirmDialog';
import { useFileStore } from '@/stores/fileStore';
import type { ScannedDirectory } from '@/types/ipc';

interface ScannedDirectoriesPanelProps {
  /** 移除成功后的回调（用于刷新文件列表等） */
  onRemoved?: () => void;
}

export function ScannedDirectoriesPanel({ onRemoved }: ScannedDirectoriesPanelProps) {
  const directories = useFileStore((s) => s.scannedDirectories);
  const loadScannedDirectories = useFileStore((s) => s.loadScannedDirectories);
  const removeDirectory = useFileStore((s) => s.removeDirectory);
  const error = useFileStore((s) => s.error);
  const clearError = useFileStore((s) => s.clearError);

  const [pendingRemove, setPendingRemove] = useState<ScannedDirectory | null>(null);
  const [removing, setRemoving] = useState(false);

  // 首次挂载加载目录列表
  useEffect(() => {
    void loadScannedDirectories();
  }, [loadScannedDirectories]);

  const handleConfirmRemove = async () => {
    if (!pendingRemove) return;
    setRemoving(true);
    try {
      await removeDirectory(pendingRemove.path);
      onRemoved?.();
    } catch {
      // 错误已写入 store.error（顶部横幅展示）
    } finally {
      setRemoving(false);
      setPendingRemove(null);
    }
  };

  if (directories.length === 0) {
    return null;
  }

  return (
    <div className="scanned-directories">
      <div className="scanned-directories__header">
        <span className="scanned-directories__title">已扫描目录</span>
        <span className="scanned-directories__count">{directories.length} 个</span>
      </div>
      <ul className="scanned-directories__list">
        {directories.map((dir) => (
          <li key={dir.id} className="scanned-directories__item">
            <div className="scanned-directories__info">
              <span className="scanned-directories__path" title={dir.path}>
                {dir.path}
              </span>
              <span className="scanned-directories__file-count">{dir.file_count} 个文件</span>
            </div>
            <button
              type="button"
              className="btn btn--ghost btn--sm"
              style={{ color: 'var(--warn)' }}
              onClick={() => setPendingRemove(dir)}
              data-testid={`remove-directory-${dir.id}`}
            >
              移除
            </button>
          </li>
        ))}
      </ul>

      {error && (
        <div className="scanned-directories__error" role="alert">
          <span>{error}</span>
          <button
            type="button"
            className="scanned-directories__error-dismiss"
            aria-label="关闭错误提示"
            onClick={clearError}
          >
            ×
          </button>
        </div>
      )}

      {pendingRemove && (
        <ConfirmDialog
          title="移除目录"
          message={`将从 FileMind 索引中移除「${pendingRemove.path}」下的 ${pendingRemove.file_count} 个文件（含向量索引）。磁盘上的文件不会被删除，重新扫描该目录即可恢复。`}
          confirmLabel="移除"
          danger
          loading={removing}
          onConfirm={() => void handleConfirmRemove()}
          onCancel={() => setPendingRemove(null)}
        />
      )}
    </div>
  );
}
