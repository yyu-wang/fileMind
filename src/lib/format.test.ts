import { describe, it, expect } from 'vitest';
import {
  formatFileSize,
  formatDateTime,
  getFileKind,
  getFileTypeMeta,
  getImageMime,
} from './format';

describe('formatFileSize', () => {
  it('formats bytes as integer', () => {
    expect(formatFileSize(0)).toBe('0 B');
    expect(formatFileSize(512)).toBe('512 B');
    expect(formatFileSize(1023)).toBe('1023 B');
  });

  it('formats KB/MB/GB with one decimal', () => {
    expect(formatFileSize(1024)).toBe('1.0 KB');
    expect(formatFileSize(1536)).toBe('1.5 KB');
    expect(formatFileSize(1024 * 1024)).toBe('1.0 MB');
    expect(formatFileSize(1024 * 1024 * 1024)).toBe('1.0 GB');
  });
});

describe('formatDateTime', () => {
  it('parses Rust UTC datetime to local YYYY-MM-DD HH:MM', () => {
    const expected = new Date('2026-08-04T10:30:00Z');
    const pad = (n: number) => String(n).padStart(2, '0');
    const want = `${expected.getFullYear()}-${pad(expected.getMonth() + 1)}-${pad(
      expected.getDate(),
    )} ${pad(expected.getHours())}:${pad(expected.getMinutes())}`;
    expect(formatDateTime('2026-08-04 10:30:00')).toBe(want);
  });

  it('returns raw string when unparseable', () => {
    expect(formatDateTime('not-a-date')).toBe('not-a-date');
  });
});

describe('getFileKind', () => {
  it('maps text extensions (case-insensitive)', () => {
    expect(getFileKind('note.md')).toBe('text');
    expect(getFileKind('main.tsx')).toBe('text');
    expect(getFileKind('README.MD')).toBe('text');
    expect(getFileKind('script.sh')).toBe('text');
  });

  it('maps image and pdf', () => {
    expect(getFileKind('photo.png')).toBe('image');
    expect(getFileKind('banner.SVG')).toBe('image');
    expect(getFileKind('doc.pdf')).toBe('pdf');
  });

  it('returns unsupported otherwise', () => {
    expect(getFileKind('archive.zip')).toBe('unsupported');
    expect(getFileKind('noext')).toBe('unsupported');
  });
});

describe('getImageMime', () => {
  it('maps known image extensions', () => {
    expect(getImageMime('png')).toBe('image/png');
    expect(getImageMime('JPG')).toBe('image/jpeg');
    expect(getImageMime('svg')).toBe('image/svg+xml');
  });

  it('falls back to octet-stream', () => {
    expect(getImageMime('xyz')).toBe('application/octet-stream');
  });
});

describe('getFileTypeMeta', () => {
  it('maps common office/code/image types', () => {
    expect(getFileTypeMeta('report.pdf')).toEqual({ label: 'PDF', kind: 'pdf' });
    expect(getFileTypeMeta('doc.docx')).toEqual({ label: 'DOC', kind: 'doc' });
    expect(getFileTypeMeta('data.csv')).toEqual({ label: 'XLS', kind: 'xls' });
    expect(getFileTypeMeta('slide.pptx')).toEqual({ label: 'PPT', kind: 'ppt' });
    expect(getFileTypeMeta('photo.png')).toEqual({ label: 'IMG', kind: 'img' });
    expect(getFileTypeMeta('README.md')).toEqual({ label: 'MD', kind: 'md' });
    expect(getFileTypeMeta('main.ts')).toEqual({ label: '{ }', kind: 'code' });
    expect(getFileTypeMeta('a.zip')).toEqual({ label: 'ZIP', kind: 'zip' });
  });

  it('is case-insensitive on extension', () => {
    expect(getFileTypeMeta('REPORT.PDF')).toEqual({ label: 'PDF', kind: 'pdf' });
  });

  it('falls back to generic file for unknown extension', () => {
    expect(getFileTypeMeta('mystery.xyz')).toEqual({ label: 'XYZ', kind: 'file' });
    expect(getFileTypeMeta('noext')).toEqual({ label: 'FILE', kind: 'file' });
  });
});
