// 格式化工具：文件大小、时间、类型与类型图标的纯函数。
//
// 设计说明：
// - formatDateTime 解析 Rust 端 UTC 格式 "YYYY-MM-DD HH:MM:SS"，转为本地时间显示
// - getFileKind 与后端 file_preview.rs 的扩展名映射保持一致
// - 扩展名数据表见 lib/fileExtensions.ts（本文件的三个判定函数共用）

import {
  CODE_EXTENSIONS,
  IMAGE_MIMES,
  MUSIC_EXTENSIONS,
  TEXT_EXTENSIONS,
  VIDEO_EXTENSIONS,
  ZIP_EXTENSIONS,
  extensionOf,
} from './fileExtensions';

export type FileKind = 'text' | 'image' | 'pdf' | 'unsupported';

const SIZE_UNITS = ['B', 'KB', 'MB', 'GB', 'TB'] as const;

const RUST_DATETIME_RE = /^\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}$/;

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
  const ext = extensionOf(fileName);
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

/** 无扩展名时的类型块。 */
const GENERIC_META: FileTypeMeta = { label: 'FILE', kind: 'file' };

/** 单条图标规则：命中任一扩展名即取该类型块。 */
interface TypeRule {
  extensions: ReadonlySet<string>;
  meta: FileTypeMeta;
}

/**
 * 类型图标规则表，按顺序判定（先命中先返回）。
 *
 * 拆成表而非 if 链的原因：原判断链有 12 个分支、8 处 `||`，圈复杂度 22；
 * 规则表把「扩展名分组」变成数据，函数体只剩查表与兜底。
 */
const TYPE_RULES: readonly TypeRule[] = [
  { extensions: new Set(['pdf']), meta: { label: 'PDF', kind: 'pdf' } },
  { extensions: new Set(['doc', 'docx']), meta: { label: 'DOC', kind: 'doc' } },
  { extensions: new Set(['xls', 'xlsx', 'csv', 'tsv']), meta: { label: 'XLS', kind: 'xls' } },
  { extensions: new Set(['ppt', 'pptx', 'odp']), meta: { label: 'PPT', kind: 'ppt' } },
  {
    extensions: new Set([...Object.keys(IMAGE_MIMES), 'heic', 'avif']),
    meta: { label: 'IMG', kind: 'img' },
  },
  { extensions: new Set(['md']), meta: { label: 'MD', kind: 'md' } },
  { extensions: new Set(['txt']), meta: { label: 'TXT', kind: 'txt' } },
  { extensions: CODE_EXTENSIONS, meta: { label: '{ }', kind: 'code' } },
  { extensions: ZIP_EXTENSIONS, meta: { label: 'ZIP', kind: 'zip' } },
  { extensions: MUSIC_EXTENSIONS, meta: { label: 'AUD', kind: 'music' } },
  { extensions: VIDEO_EXTENSIONS, meta: { label: 'VID', kind: 'video' } },
];

/** 按文件名判定类型图标；无扩展名或未知扩展名回退 generic 文件块。 */
export function getFileTypeMeta(fileName: string): FileTypeMeta {
  const ext = extensionOf(fileName);
  if (!ext) {
    return GENERIC_META;
  }
  const rule = TYPE_RULES.find((item) => item.extensions.has(ext));
  // 未知扩展名：回退 generic 文件块（显示扩展名前 3 位大写）
  return rule ? rule.meta : { label: ext.slice(0, 3).toUpperCase(), kind: 'file' };
}
