// cloudProviderValidation 单元测试：五个字段的校验规则与整表校验（含编辑态跳过 slug）。

import { describe, expect, it } from 'vitest';

import {
  validateBaseUrl,
  validateName,
  validateProviderForm,
  validateRemark,
  validateSlug,
  validateWebsite,
  type CloudProviderFormValues,
} from './cloudProviderValidation';

function values(overrides: Partial<CloudProviderFormValues> = {}): CloudProviderFormValues {
  return {
    slug: 'qwen',
    name: 'DashScope',
    remark: '',
    website: '',
    baseUrl: 'https://dashscope.aliyuncs.com/compatible-mode/v1',
    ...overrides,
  };
}

describe('validateSlug', () => {
  it('接受小写字母、数字、下划线与短横', () => {
    expect(validateSlug('qwen-2_turbo')).toBeNull();
    expect(validateSlug('a')).toBeNull();
  });

  it('拒绝空串、大写、空格与超长', () => {
    expect(validateSlug('')).toMatch(/标识仅允许/);
    expect(validateSlug('Qwen')).toMatch(/标识仅允许/);
    expect(validateSlug('a b')).toMatch(/标识仅允许/);
    expect(validateSlug('a'.repeat(65))).toMatch(/标识仅允许/);
  });
});

describe('validateName', () => {
  it('必填，边界为 128 字符', () => {
    expect(validateName('')).toBe('显示名称不能为空');
    expect(validateName('a'.repeat(128))).toBeNull();
    expect(validateName('a'.repeat(129))).toBe('显示名称不能超过 128 字符');
  });
});

describe('validateRemark', () => {
  it('可空，超 255 字符报错', () => {
    expect(validateRemark('')).toBeNull();
    expect(validateRemark('a'.repeat(255))).toBeNull();
    expect(validateRemark('a'.repeat(256))).toBe('备注不能超过 255 字符');
  });
});

describe('validateWebsite', () => {
  it('留空通过，非 https 报错', () => {
    expect(validateWebsite('')).toBeNull();
    expect(validateWebsite('https://help.aliyun.com')).toBeNull();
    expect(validateWebsite('http://help.aliyun.com')).toBe('官网链接必须是 https:// 或留空');
  });
});

describe('validateBaseUrl', () => {
  it('必填，且不以「/」结尾', () => {
    expect(validateBaseUrl('')).toBe('请求地址不能为空');
    expect(validateBaseUrl('https://example.com/v1/')).toBe('请求地址不能以「/」结尾');
  });

  it('接受 https 与本地 http 调试地址', () => {
    expect(validateBaseUrl('https://example.com/v1')).toBeNull();
    expect(validateBaseUrl('http://localhost:8080/v1')).toBeNull();
    expect(validateBaseUrl('http://127.0.0.1/v1')).toBeNull();
  });

  it('拒绝非本地的明文 http', () => {
    expect(validateBaseUrl('http://example.com/v1')).toMatch(/必须是 https/);
  });
});

describe('validateProviderForm', () => {
  it('新建模式校验 slug', () => {
    const errors = validateProviderForm(values({ slug: 'Bad Slug' }), { isEdit: false });
    expect(errors.slug).toMatch(/标识仅允许/);
  });

  it('编辑模式跳过 slug（创建后不可改）', () => {
    const errors = validateProviderForm(values({ slug: 'Bad Slug' }), { isEdit: true });
    expect(errors.slug).toBeNull();
    expect(Object.values(errors).every((message) => message === null)).toBe(true);
  });

  it('逐字段上报错误，通过的字段为 null', () => {
    const errors = validateProviderForm(values({ name: '', baseUrl: '' }), { isEdit: false });
    expect(errors.name).toBe('显示名称不能为空');
    expect(errors.baseUrl).toBe('请求地址不能为空');
    expect(errors.remark).toBeNull();
    expect(errors.website).toBeNull();
  });
});
