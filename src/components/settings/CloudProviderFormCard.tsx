// P-07：自定义云提供商表单卡片（设置页）。
//
// 供用户创建/编辑 cloud_providers 表记录。字段约束与 Rust 端校验一致：
// - provider_key（slug）：`[a-z0-9_-]{1,64}`，创建后不可变（编辑时禁用编辑）
// - name：1-128 字符
// - remark：0-255 字符
// - base_url：合法 https URL，不能以 / 结尾（http 仅允许 localhost/127.0.0.1）
// - website：可选 https URL 或留空
//
// 提交由 props.onSubmit 调用方统一处理（通常是 upsertCloudProvider），本卡片
// 不直接绑定 store，便于在「新建对话框」与「编辑行内」两种场景复用。

import { useMemo, useState, type FormEvent } from 'react';
import type { CloudProviderRecord, CloudProviderUpsertInput } from '../../types/ipc';

export interface CloudProviderFormCardProps {
  /** 初始值（编辑时传；不传则视为新建） */
  initial?: CloudProviderRecord | null;
  /** 提交回调（校验通过后触发；所有字段均为后端期望的规范化形态） */
  onSubmit: (input: CloudProviderUpsertInput) => Promise<void> | void;
  /** 取消（仅编辑模式展示；点取消不触发 onSubmit） */
  onCancel?: () => void;
  /** 提交中 loading */
  submitting?: boolean;
}

//: slug 规则与 Rust validate_provider_key 对齐
const SLUG_RE = /^[a-z0-9_-]{1,64}$/;
//: URL 带协议段的最低校验（表单层只做结构校验，入库前 Rust 侧做完整校验）
const HTTPS_URL_RE = /^https:\/\/[^\s/]+/i;
const LOCALHOST_HTTP_RE = /^http:\/\/(localhost|127\.0\.0\.1)(:\d+)?(\/|$)/i;

function validateBaseUrl(url: string): string | null {
  if (!url) return '请求地址不能为空';
  if (url.endsWith('/')) return '请求地址不能以「/」结尾';
  if (HTTPS_URL_RE.test(url)) return null;
  if (LOCALHOST_HTTP_RE.test(url)) return null;
  return '必须是 https:// 地址（本地调试可用 http://localhost/127.0.0.1）';
}

function validateWebsite(url: string | null): string | null {
  if (!url) return null;
  if (HTTPS_URL_RE.test(url)) return null;
  return '官网链接必须是 https:// 或留空';
}

function validateName(name: string): string | null {
  if (!name) return '显示名称不能为空';
  if (name.length > 128) return '显示名称不能超过 128 字符';
  return null;
}

function validateRemark(remark: string | null): string | null {
  if (remark === null || remark === undefined) return null;
  if (remark.length > 255) return '备注不能超过 255 字符';
  return null;
}

export function CloudProviderFormCard({
  initial = null,
  onSubmit,
  onCancel,
  submitting = false,
}: CloudProviderFormCardProps) {
  const isEdit = Boolean(initial);
  // 表单状态完全由 initial 派生；要切换编辑对象，调用方传不同 provider_key 时会
  // 触发 CloudProviderManager 用 `key` 强制 remount 组件，因此不需要 effect
  // 回填，也不会出现「initial 异步到达但 state 已旧」的问题。
  const [slug, setSlug] = useState(initial?.provider_key ?? '');
  const [name, setName] = useState(initial?.name ?? '');
  const [remark, setRemark] = useState(initial?.remark ?? '');
  const [website, setWebsite] = useState(initial?.website ?? '');
  const [baseUrl, setBaseUrl] = useState(initial?.base_url ?? '');
  const [touched, setTouched] = useState<Record<string, boolean>>({});
  const [formError, setFormError] = useState<string | null>(null);

  const errors = useMemo(() => {
    return {
      slug: isEdit
        ? null
        : SLUG_RE.test(slug)
          ? null
          : '标识仅允许小写字母/数字/下划线/短横（1-64 字符）',
      name: validateName(name),
      remark: validateRemark(remark || null),
      website: validateWebsite(website || null),
      baseUrl: validateBaseUrl(baseUrl),
    };
  }, [slug, name, remark, website, baseUrl, isEdit]);

  const isValid = !Object.values(errors).some(Boolean);

  const touch = (field: string) => {
    setTouched((t) => ({ ...t, [field]: true }));
  };

  const handleSubmit = async (evt: FormEvent<HTMLElement>) => {
    evt.preventDefault();
    // 首次提交时全量标记 touched，错误一次展示
    setTouched({ slug: true, name: true, remark: true, website: true, baseUrl: true });
    if (!isValid) {
      setFormError('请修正下方表单错误');
      return;
    }
    setFormError(null);
    try {
      await onSubmit({
        provider_key: slug,
        name,
        remark: remark || null,
        website: website || null,
        base_url: baseUrl,
      });
    } catch (e) {
      setFormError(e instanceof Error ? e.message : '保存提供商失败');
    }
  };

  const inputCls = (field: keyof typeof errors) =>
    `input ${touched[field] && errors[field] ? 'input--error' : ''}`;

  return (
    <form
      className={`settings-card cloud-provider-form ${isEdit ? 'cloud-provider-form--edit' : ''}`}
      onSubmit={handleSubmit}
      aria-labelledby="cloud-provider-form-title"
    >
      <h4 id="cloud-provider-form-title" className="settings-card__title">
        {isEdit ? '编辑云提供商' : '添加云提供商'}
      </h4>
      <p className="section-desc">
        填写 OpenAI 兼容 API 的基础信息，Key 通过上方「AI 模型配置」单独写入系统 Keychain。
      </p>

      <div className="setting-row">
        <div className="setting-label">
          <div className="name">提供商标识（slug）</div>
          <div className="desc">全小写、用于路由匹配，创建后不可修改</div>
        </div>
        <div className="setting-control">
          <input
            className={inputCls('slug')}
            value={slug}
            disabled={isEdit || submitting}
            onChange={(e) => setSlug(e.target.value.toLowerCase())}
            onBlur={() => touch('slug')}
            placeholder="如 qwen / moonshot / glm"
            aria-label="提供商标识 slug"
            style={{ width: 260 }}
          />
          {touched.slug && errors.slug && (
            <p className="settings-section__error" role="alert" style={{ marginTop: 4 }}>
              {errors.slug}
            </p>
          )}
        </div>
      </div>

      <div className="setting-row">
        <div className="setting-label">
          <div className="name">显示名称</div>
          <div className="desc">卡片/菜单展示用（如「阿里通义千问」）</div>
        </div>
        <div className="setting-control">
          <input
            className={inputCls('name')}
            value={name}
            disabled={submitting}
            onChange={(e) => setName(e.target.value)}
            onBlur={() => touch('name')}
            placeholder="如 DashScope / Claude API"
            aria-label="提供商显示名称"
            style={{ width: 260 }}
          />
          {touched.name && errors.name && (
            <p className="settings-section__error" role="alert" style={{ marginTop: 4 }}>
              {errors.name}
            </p>
          )}
        </div>
      </div>

      <div className="setting-row">
        <div className="setting-label">
          <div className="name">请求地址 Base URL</div>
          <div className="desc">
            OpenAI 兼容前缀，以 /chat/completions 之前的部分为准，不含尾斜杠
          </div>
        </div>
        <div className="setting-control">
          <input
            className={inputCls('baseUrl')}
            value={baseUrl}
            disabled={submitting}
            onChange={(e) => setBaseUrl(e.target.value.trimEnd())}
            onBlur={() => touch('baseUrl')}
            placeholder="https://dashscope.aliyuncs.com/compatible-mode/v1"
            aria-label="Base URL"
            style={{ width: 420 }}
          />
          {touched.baseUrl && errors.baseUrl && (
            <p className="settings-section__error" role="alert" style={{ marginTop: 4 }}>
              {errors.baseUrl}
            </p>
          )}
        </div>
      </div>

      <div className="setting-row">
        <div className="setting-label">
          <div className="name">备注</div>
          <div className="desc">如「公司专用代理」「测试环境」（仅 UI 展示，可空）</div>
        </div>
        <div className="setting-control">
          <input
            className={inputCls('remark')}
            value={remark}
            disabled={submitting}
            onChange={(e) => setRemark(e.target.value)}
            onBlur={() => touch('remark')}
            placeholder="可选，最长 255 字"
            aria-label="备注"
            style={{ width: 420 }}
          />
          {touched.remark && errors.remark && (
            <p className="settings-section__error" role="alert" style={{ marginTop: 4 }}>
              {errors.remark}
            </p>
          )}
        </div>
      </div>

      <div className="setting-row" style={{ borderBottom: 'none' }}>
        <div className="setting-label">
          <div className="name">官网链接</div>
          <div className="desc">可选，仅用于跳转查看文档，留空不展示</div>
        </div>
        <div className="setting-control">
          <input
            className={inputCls('website')}
            value={website}
            disabled={submitting}
            onChange={(e) => setWebsite(e.target.value.trim())}
            onBlur={() => touch('website')}
            placeholder="https://help.aliyun.com/..."
            aria-label="官网链接"
            style={{ width: 420 }}
          />
          {touched.website && errors.website && (
            <p className="settings-section__error" role="alert" style={{ marginTop: 4 }}>
              {errors.website}
            </p>
          )}
        </div>
      </div>

      {formError && (
        <p className="settings-section__error" role="alert">
          {formError}
        </p>
      )}

      <div
        className="settings-card__actions"
        style={{ display: 'flex', gap: 8, justifyContent: 'flex-end' }}
      >
        {onCancel && (
          <button
            type="button"
            className="btn btn--ghost btn--sm"
            disabled={submitting}
            onClick={onCancel}
          >
            取消
          </button>
        )}
        <button type="submit" className="btn btn--primary btn--sm" disabled={submitting}>
          {submitting ? '保存中...' : isEdit ? '保存修改' : '添加提供商'}
        </button>
      </div>
    </form>
  );
}
