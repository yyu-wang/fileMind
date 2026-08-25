// 格式化工具：文件大小、时间、类型的纯函数。
//
// 设计说明：
// - formatDateTime 解析 Rust 端 UTC 格式 "YYYY-MM-DD HH:MM:SS"，转为本地时间显示
// - getFileKind 与后端 file_preview.rs 的扩展名映射保持一致

export type FileKind = 'text' | 'image' | 'pdf' | 'unsupported';

const SIZE_UNITS = ['B', 'KB', 'MB', 'GB', 'TB'] as const;

const RUST_DATETIME_RE = /^\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}$/;

const TEXT_EXTENSIONS = new Set([
  'txt',
  'md',
  'log',
  'json',
  'yaml',
  'yml',
  'csv',
  'xml',
  'toml',
  'ini',
  'conf',
  'ts',
  'tsx',
  'js',
  'jsx',
  'py',
  'rs',
  'go',
  'java',
  'c',
  'h',
  'cpp',
  'css',
  'html',
  'sh',
  'sql',
]);

const IMAGE_MIMES: Record<string, string | undefined> = {
  png: 'image/png',
  jpg: 'image/jpeg',
  jpeg: 'image/jpeg',
  gif: 'image/gif',
  webp: 'image/webp',
  svg: 'image/svg+xml',
  bmp: 'image/bmp',
  ico: 'image/x-icon',
};

/** 文件大小字节 → 人类可读形式（B 取整，KB/MB/GB 保留 1 位小数）。 */
export function formatFileSize(bytes: number): string {
  let value = bytes;
  let unitIndex = 0;
  while (value >= 1024 && unitIndex < SIZE_UNITS.length - 1) {
    value /= 1024;
    unitIndex += 1;
  }
  if (unitIndex === 0) {
    return `${Math.round(value)} B`;
  }
  return `${value.toFixed(1)} ${SIZE_UNITS[unitIndex]}`;
}

/** Rust UTC 时间串 → 本地 `YYYY-MM-DD HH:MM`；无法解析时原样返回。 */
export function formatDateTime(ts: string): string {
  const normalized = RUST_DATETIME_RE.test(ts) ? `${ts.replace(' ', 'T')}Z` : ts;
  const date = new Date(normalized);
  if (Number.isNaN(date.getTime())) {
    return ts;
  }
  const pad = (n: number) => String(n).padStart(2, '0');
  const yyyy = date.getFullYear();
  const mm = pad(date.getMonth() + 1);
  const dd = pad(date.getDate());
  const hh = pad(date.getHours());
  const mi = pad(date.getMinutes());
  return `${yyyy}-${mm}-${dd} ${hh}:${mi}`;
}

/** 按扩展名判定预览类型，映射与后端 file_preview.rs 的 classify_extension 一致。 */
export function getFileKind(fileName: string): FileKind {
  const ext = fileName.split('.').pop()?.toLowerCase();
  if (!ext) {
    return 'unsupported';
  }
  if (TEXT_EXTENSIONS.has(ext)) {
    return 'text';
  }
  if (ext === 'pdf') {
    return 'pdf';
  }
  if (ext in IMAGE_MIMES) {
    return 'image';
  }
  return 'unsupported';
}

/** 图片扩展名 → MIME；未知扩展名回退 octet-stream。 */
export function getImageMime(ext: string): string {
  return IMAGE_MIMES[ext.toLowerCase()] ?? 'application/octet-stream';
}

/**
 * 文件类型图标（对齐交互原型 §文件管理 表格的彩色类型块）。
 *
 * 按扩展名分组映射到图标标签与视觉类别，类别决定 CSS 配色（.file-type-icon--{kind}）。
 */
export type FileTypeKind =
  | 'pdf'
  | 'doc'
  | 'xls'
  | 'ppt'
  | 'img'
  | 'md'
  | 'txt'
  | 'code'
  | 'zip'
  | 'music'
  | 'video'
  | 'file';

export interface FileTypeMeta {
  /** 图标块内文本（对齐原型 PDF/DOC/IMG/{ }/MD/XLS） */
  label: string;
  /** 视觉类别（CSS 配色用） */
  kind: FileTypeKind;
}

const CODE_EXTENSIONS = new Set([
  'ts',
  'tsx',
  'js',
  'jsx',
  'py',
  'rs',
  'java',
  'c',
  'h',
  'cpp',
  'css',
  'html',
  'sh',
  'sql',
  'go',
  'json',
  'yaml',
  'yml',
  'toml',
  'xml',
  'log',
]);

const ZIP_EXTENSIONS = new Set(['zip', 'rar', '7z', 'tar', 'gz', 'bz2', 'xz']);
const MUSIC_EXTENSIONS = new Set(['mp3', 'wav', 'flac', 'aac', 'ogg', 'm4a']);
const VIDEO_EXTENSIONS = new Set(['mp4', 'mkv', 'mov', 'avi', 'wmv', 'flv', 'webm', 'm4v']);

/** 按文件名判定类型图标；无扩展名/未知扩展名回退 generic 文件块。 */
export function getFileTypeMeta(fileName: string): FileTypeMeta {
  const dot = fileName.lastIndexOf('.');
  const ext = dot >= 0 ? fileName.slice(dot + 1).toLowerCase() : '';
  if (!ext) return { label: 'FILE', kind: 'file' };
  if (ext === 'pdf') return { label: 'PDF', kind: 'pdf' };
  if (ext === 'doc' || ext === 'docx') return { label: 'DOC', kind: 'doc' };
  if (ext === 'xls' || ext === 'xlsx' || ext === 'csv' || ext === 'tsv') {
    return { label: 'XLS', kind: 'xls' };
  }
  if (ext === 'ppt' || ext === 'pptx' || ext === 'odp') return { label: 'PPT', kind: 'ppt' };
  if (IMAGE_MIMES[ext] || ext === 'heic' || ext === 'avif') return { label: 'IMG', kind: 'img' };
  if (ext === 'md') return { label: 'MD', kind: 'md' };
  if (ext === 'txt') return { label: 'TXT', kind: 'txt' };
  if (CODE_EXTENSIONS.has(ext)) return { label: '{ }', kind: 'code' };
  if (ZIP_EXTENSIONS.has(ext)) return { label: 'ZIP', kind: 'zip' };
  if (MUSIC_EXTENSIONS.has(ext)) return { label: 'AUD', kind: 'music' };
  if (VIDEO_EXTENSIONS.has(ext)) return { label: 'VID', kind: 'video' };
  // 未知扩展名：回退 generic 文件块（显示扩展名前 3 位大写）
  return { label: ext.slice(0, 3).toUpperCase(), kind: 'file' };
}
