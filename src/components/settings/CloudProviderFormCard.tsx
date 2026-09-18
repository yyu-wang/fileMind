// P-07：自定义云提供商表单卡片（设置页）。
//
// 供用户创建/编辑 cloud_providers 表记录。字段约束见 lib/cloudProviderValidation.ts，
// 与 Rust 端校验一致：slug `[a-z0-9_-]{1,64}`（创建后不可修改）、name 1-128 字符、
// remark 0-255 字符、base_url 合法 https 且不以 / 结尾（http 仅限 localhost/127.0.0.1）、
// website 可选 https。
//
// 提交由 props.onSubmit 调用方统一处理（通常是 upsertCloudProvider），本卡片不直接
// 绑定 store，便于在「新建对话框」与「编辑行内」两种场景复用。表单状态收在
// useCloudProviderForm 里，本组件只负责拼装字段行。

import { SettingsInputRow } from '@/components/ui/SettingsInputRow';
import type { CloudProviderRecord, CloudProviderUpsertInput } from '@/types/ipc';

import { useCloudProviderForm } from './useCloudProviderForm';

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

/** 卡片标题随模式变化。 */
function formTitle(isEdit: boolean): string {
  return isEdit ? '编辑云提供商' : '添加云提供商';
}

/** 提交按钮文案：提交中 > 编辑 > 新建。 */
function submitLabel(submitting: boolean, isEdit: boolean): string {
  if (submitting) return '保存中...';
  return isEdit ? '保存修改' : '添加提供商';
}

export function CloudProviderFormCard({
  initial = null,
  onSubmit,
  onCancel,
  submitting = false,
}: CloudProviderFormCardProps) {
  const form = useCloudProviderForm({ initial, onSubmit });

  return (
    <form
      className={`settings-card cloud-provider-form${form.isEdit ? ' cloud-provider-form--edit' : ''}`}
      onSubmit={form.handleSubmit}
      aria-labelledby="cloud-provider-form-title"
    >
      <h4 id="cloud-provider-form-title" className="settings-card__title">
        {formTitle(form.isEdit)}
      </h4>
      <p className="section-desc">
        填写 OpenAI 兼容 API 的基础信息，Key 通过上方「AI 模型配置」单独写入系统 Keychain。
      </p>

      <SettingsInputRow
        label="提供商标识（slug）"
        desc="全小写、用于路由匹配，创建后不可修改"
        value={form.values.slug}
        onChange={(value) => form.setField('slug', value.toLowerCase())}
        onBlur={() => form.touch('slug')}
        error={form.errorFor('slug')}
        disabled={form.isEdit || submitting}
        placeholder="如 qwen / moonshot / glm"
        ariaLabel="提供商标识 slug"
      />

      <SettingsInputRow
        label="显示名称"
        desc="卡片/菜单展示用（如「阿里通义千问」）"
        value={form.values.name}
        onChange={(value) => form.setField('name', value)}
        onBlur={() => form.touch('name')}
        error={form.errorFor('name')}
        disabled={submitting}
        placeholder="如 DashScope / Claude API"
        ariaLabel="提供商显示名称"
      />

      <SettingsInputRow
        label="请求地址 Base URL"
        desc="OpenAI 兼容前缀，以 /chat/completions 之前的部分为准，不含尾斜杠"
        value={form.values.baseUrl}
        onChange={(value) => form.setField('baseUrl', value.trimEnd())}
        onBlur={() => form.touch('baseUrl')}
        error={form.errorFor('baseUrl')}
        disabled={submitting}
        placeholder="https://dashscope.aliyuncs.com/compatible-mode/v1"
        ariaLabel="Base URL"
        width={420}
      />

      <SettingsInputRow
        label="备注"
        desc="如「公司专用代理」「测试环境」（仅 UI 展示，可空）"
        value={form.values.remark}
        onChange={(value) => form.setField('remark', value)}
        onBlur={() => form.touch('remark')}
        error={form.errorFor('remark')}
        disabled={submitting}
        placeholder="可选，最长 255 字"
        ariaLabel="备注"
        width={420}
      />

      <SettingsInputRow
        label="官网链接"
        desc="可选，仅用于跳转查看文档，留空不展示"
        value={form.values.website}
        onChange={(value) => form.setField('website', value.trim())}
        onBlur={() => form.touch('website')}
        error={form.errorFor('website')}
        disabled={submitting}
        placeholder="https://help.aliyun.com/..."
        ariaLabel="官网链接"
        width={420}
        last
      />

      {form.formError && (
        <p className="settings-section__error" role="alert">
          {form.formError}
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
          {submitLabel(submitting, form.isEdit)}
        </button>
      </div>
    </form>
  );
}
