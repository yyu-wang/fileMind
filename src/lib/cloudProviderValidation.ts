// 云提供商表单的字段校验（纯函数）。
//
// 规则与 Rust 端保持一致（slug 对应 `validate_provider_key`）：表单层只做结构校验，
// 入库前 Rust 侧仍会完整校验一次；两边规则若改动需同步，否则会出现「前端放行、
// 后端拒绝」的错位。空串在这里表示「未填写」，与提交 payload 里的 null 相区分——
// 转 null 的动作由调用方在组 payload 时完成。

/** slug 规则与 Rust `validate_provider_key` 对齐。 */
const SLUG_RE = /^[a-z0-9_-]{1,64}$/;
/** URL 带协议段的最低校验。 */
const HTTPS_URL_RE = /^https:\/\/[^\s/]+/i;
/** 仅 localhost / 127.0.0.1 允许明文 http（本地调试）。 */
const LOCALHOST_HTTP_RE = /^http:\/\/(localhost|127\.0\.0\.1)(:\d+)?(\/|$)/i;

/** 表单各字段的原始值（空串表示未填写）。 */
export interface CloudProviderFormValues {
  slug: string;
  name: string;
  remark: string;
  website: string;
  baseUrl: string;
}

/** 各字段的错误文案，null 表示通过。 */
export interface CloudProviderFormErrors {
  slug: string | null;
  name: string | null;
  remark: string | null;
  website: string | null;
  baseUrl: string | null;
}

/** 提供商标识：仅小写字母、数字、下划线、短横，1-64 字符。 */
export function validateSlug(slug: string): string | null {
  return SLUG_RE.test(slug) ? null : '标识仅允许小写字母/数字/下划线/短横（1-64 字符）';
}

/** 显示名称：必填，最长 128 字符。 */
export function validateName(name: string): string | null {
  if (!name) return '显示名称不能为空';
  if (name.length > 128) return '显示名称不能超过 128 字符';
  return null;
}

/** 备注：可空，最长 255 字符。 */
export function validateRemark(remark: string): string | null {
  if (remark.length > 255) return '备注不能超过 255 字符';
  return null;
}

/** 官网链接：可空，填写时必须是 https。 */
export function validateWebsite(website: string): string | null {
  if (!website) return null;
  return HTTPS_URL_RE.test(website) ? null : '官网链接必须是 https:// 或留空';
}

/** 请求地址：必填，https（本地调试可 http://localhost 或 127.0.0.1），且不以 / 结尾。 */
export function validateBaseUrl(baseUrl: string): string | null {
  if (!baseUrl) return '请求地址不能为空';
  if (baseUrl.endsWith('/')) return '请求地址不能以「/」结尾';
  if (HTTPS_URL_RE.test(baseUrl)) return null;
  if (LOCALHOST_HTTP_RE.test(baseUrl)) return null;
  return '必须是 https:// 地址（本地调试可用 http://localhost/127.0.0.1）';
}

/**
 * 校验整个表单。
 *
 * Args:
 *   values: 各字段当前值
 *   options: isEdit=true 时跳过 slug 校验（slug 创建后不可改，输入框也禁用）
 *
 * Returns:
 *   每个字段的错误文案，null 表示通过
 */
export function validateProviderForm(
  values: CloudProviderFormValues,
  { isEdit }: { isEdit: boolean },
): CloudProviderFormErrors {
  return {
    slug: isEdit ? null : validateSlug(values.slug),
    name: validateName(values.name),
    remark: validateRemark(values.remark),
    website: validateWebsite(values.website),
    baseUrl: validateBaseUrl(values.baseUrl),
  };
}
