// fileExtensions 单元测试：扩展名解析的边界（无点、点结尾、隐藏文件、多点）。

import { describe, expect, it } from 'vitest';

import { extensionOf } from './fileExtensions';

describe('extensionOf', () => {
  it('取最后一个点之后并转小写', () => {
    expect(extensionOf('Report.PDF')).toBe('pdf');
    expect(extensionOf('backup.tar.gz')).toBe('gz');
  });

  it('无点、点结尾或空串时返回空串', () => {
    expect(extensionOf('noext')).toBe('');
    expect(extensionOf('trailing.')).toBe('');
    expect(extensionOf('')).toBe('');
  });

  it('隐藏文件视其名为扩展名（与 split(".").pop() 语义一致）', () => {
    expect(extensionOf('.gitignore')).toBe('gitignore');
  });

  it('只认文件名末段（含路径时同样成立）', () => {
    expect(extensionOf('/tmp/a.b/report.md')).toBe('md');
  });
});
