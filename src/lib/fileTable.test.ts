import { describe, it, expect } from 'vitest';
import type { FileInfo } from '@/types/ipc';
import {
  categoryTagClass,
  deriveFileStatus,
  filterByName,
  filterFiles,
  sortFiles,
} from './fileTable';

function makeFile(overrides: Partial<FileInfo>): FileInfo {
  return {
    id: 'id-1',
    path: '/tmp/f.txt',
    file_name: 'f.txt',
    file_size: 100,
    content_hash: null,
    category: null,
    created_at: '2026-08-01 00:00:00',
    updated_at: '2026-08-01 00:00:00',
    ...overrides,
  };
}

describe('deriveFileStatus', () => {
  it('marks null category as uncategorized', () => {
    expect(deriveFileStatus(makeFile({ category: null }))).toBe('uncategorized');
  });

  it('marks non-null category as categorized', () => {
    expect(deriveFileStatus(makeFile({ category: '财务' }))).toBe('categorized');
  });
});

describe('filterFiles', () => {
  const files = [
    makeFile({ id: '1', file_name: 'a.txt', category: '财务' }),
    makeFile({ id: '2', file_name: 'b.txt', category: '市场' }),
    makeFile({ id: '3', file_name: 'c.txt', category: null }),
  ];

  it('filters by category', () => {
    const result = filterFiles(files, { category: '财务', status: null });
    expect(result.map((f) => f.id)).toEqual(['1']);
  });

  it('filters by status', () => {
    const result = filterFiles(files, { category: null, status: 'uncategorized' });
    expect(result.map((f) => f.id)).toEqual(['3']);
  });

  it('combines category and status', () => {
    const result = filterFiles(files, { category: '市场', status: 'categorized' });
    expect(result.map((f) => f.id)).toEqual(['2']);
  });

  it('returns all when no filter', () => {
    const result = filterFiles(files, { category: null, status: null });
    expect(result).toHaveLength(3);
  });
});

describe('sortFiles', () => {
  it('sorts by name with numeric collation', () => {
    const files = [
      makeFile({ id: '1', file_name: 'b.txt' }),
      makeFile({ id: '2', file_name: 'a.txt' }),
      makeFile({ id: '3', file_name: 'a10.txt' }),
      makeFile({ id: '4', file_name: 'a2.txt' }),
    ];
    const sorted = sortFiles(files, 'name', 'asc');
    expect(sorted.map((f) => f.file_name)).toEqual(['a.txt', 'a2.txt', 'a10.txt', 'b.txt']);
  });

  it('sorts by size asc/desc', () => {
    const files = [
      makeFile({ id: '1', file_size: 30 }),
      makeFile({ id: '2', file_size: 10 }),
      makeFile({ id: '3', file_size: 20 }),
    ];
    expect(sortFiles(files, 'size', 'asc').map((f) => f.file_size)).toEqual([10, 20, 30]);
    expect(sortFiles(files, 'size', 'desc').map((f) => f.file_size)).toEqual([30, 20, 10]);
  });

  it('sorts by updated_at asc/desc', () => {
    const files = [
      makeFile({ id: '1', updated_at: '2026-08-05 10:00:00' }),
      makeFile({ id: '2', updated_at: '2026-08-03 10:00:00' }),
      makeFile({ id: '3', updated_at: '2026-08-04 10:00:00' }),
    ];
    expect(sortFiles(files, 'time', 'asc').map((f) => f.id)).toEqual(['2', '3', '1']);
    expect(sortFiles(files, 'time', 'desc').map((f) => f.id)).toEqual(['1', '3', '2']);
  });

  it('does not mutate the input array', () => {
    const files = [
      makeFile({ id: '1', file_name: 'b.txt' }),
      makeFile({ id: '2', file_name: 'a.txt' }),
    ];
    const before = files.map((f) => f.file_name);
    sortFiles(files, 'name', 'asc');
    expect(files.map((f) => f.file_name)).toEqual(before);
  });
});

describe('filterByName', () => {
  const files = [
    makeFile({ id: '1', file_name: 'Q2_营收报告.pdf' }),
    makeFile({ id: '2', file_name: 'meeting_notes.md' }),
    makeFile({ id: '3', file_name: '预算表_2026.xlsx' }),
  ];

  it('filters by keyword (case-insensitive)', () => {
    expect(filterByName(files, 'Q2').map((f) => f.id)).toEqual(['1']);
    expect(filterByName(files, 'q2').map((f) => f.id)).toEqual(['1']);
  });

  it('matches partial filename', () => {
    expect(filterByName(files, '2026').map((f) => f.id)).toEqual(['3']);
  });

  it('returns all when query is blank', () => {
    expect(filterByName(files, '')).toHaveLength(3);
    expect(filterByName(files, '   ')).toHaveLength(3);
  });

  it('returns empty when no match', () => {
    expect(filterByName(files, '不存在')).toHaveLength(0);
  });
});

describe('categoryTagClass', () => {
  it('returns a stable tag color for the same category name', () => {
    expect(categoryTagClass('财务')).toBe(categoryTagClass('财务'));
  });

  it('returns tag--{color} class names', () => {
    const cls = categoryTagClass('市场');
    expect(cls.startsWith('tag tag--')).toBe(true);
  });
});
