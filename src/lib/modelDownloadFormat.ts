// 模型下载状态的展示格式化（设置页 Embedding / Rerank / 本地 GGUF 三个区块共用）。
//
// 独立成 lib 模块的原因：同一套「字节 → MB / 百分比 / 进度文案」口径原先在多个区块各存
// 一份，口径一旦分叉，同一个下载进度在不同卡片上会显示不一致，故收敛到一处。

import type { ModelDownloadStatus } from '@/types/ipc';

/** 字节数 → MB 文案（1 位小数）。 */
export function formatMb(bytes: number): string {
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

/** 下载百分比；总量未知（镜像不返回大小）时返回 null。 */
export function percentOf(download: ModelDownloadStatus): number | null {
  if (download.total_bytes == null || download.total_bytes <= 0) return null;
  return Math.floor((download.downloaded_bytes / download.total_bytes) * 100);
}

/** 下载文案：百分比 + 已下载量（总量未知时退化为「已下载 x MB」）。 */
export function progressText(download: ModelDownloadStatus): string {
  const percent = percentOf(download);
  if (percent === null) return `已下载 ${formatMb(download.downloaded_bytes)}`;
  return `${percent}%（${formatMb(download.downloaded_bytes)} / ${formatMb(download.total_bytes ?? 0)}）`;
}
