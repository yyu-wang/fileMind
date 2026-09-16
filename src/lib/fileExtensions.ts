// 文件扩展名数据表：预览类型与类型图标所需的扩展名分组、图片 MIME 映射。
//
// 从 format.ts 抽出——这些表是纯数据（占了原文件约三分之一篇幅），消费方只有 format.ts
// 一处；分开后两边都落在行数阈值内，表本身也更容易与后端对照。
//
// 与后端保持一致的映射：TEXT_EXTENSIONS 对应 file_preview.rs 的文本判定，
// IMAGE_MIMES 对应其图片 MIME 表；改动需两侧同步。

/** 图片扩展名 → MIME。 */
export const IMAGE_MIMES: Record<string, string | undefined> = {
  png: 'image/png',
  jpg: 'image/jpeg',
  jpeg: 'image/jpeg',
  gif: 'image/gif',
  webp: 'image/webp',
  svg: 'image/svg+xml',
  bmp: 'image/bmp',
  ico: 'image/x-icon',
};

/** 预览为纯文本的扩展名。 */
export const TEXT_EXTENSIONS: ReadonlySet<string> = new Set([
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

/** 类型图标归为「代码」的扩展名。 */
export const CODE_EXTENSIONS: ReadonlySet<string> = new Set([
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

/** 压缩包扩展名。 */
export const ZIP_EXTENSIONS: ReadonlySet<string> = new Set([
  'zip',
  'rar',
  '7z',
  'tar',
  'gz',
  'bz2',
  'xz',
]);

/** 音频扩展名。 */
export const MUSIC_EXTENSIONS: ReadonlySet<string> = new Set([
  'mp3',
  'wav',
  'flac',
  'aac',
  'ogg',
  'm4a',
]);

/** 视频扩展名。 */
export const VIDEO_EXTENSIONS: ReadonlySet<string> = new Set([
  'mp4',
  'mkv',
  'mov',
  'avi',
  'wmv',
  'flv',
  'webm',
  'm4v',
]);

/**
 * 取文件名最后一个点之后的小写扩展名。
 *
 * Args:
 *   fileName: 文件名或路径（点开头的隐藏文件视为「扩展名即其名」）
 *
 * Returns:
 *   小写扩展名；无点、点结尾或空串时返回空串
 */
export function extensionOf(fileName: string): string {
  const dot = fileName.lastIndexOf('.');
  return dot >= 0 ? fileName.slice(dot + 1).toLowerCase() : '';
}
