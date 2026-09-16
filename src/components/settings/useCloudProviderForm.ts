// 云提供商表单的状态与提交：字段值、touched、校验结果与错误文案。
//
// 从 CloudProviderFormCard 抽出——卡片的复杂度此前为 30，其中五处空值合并与提交链路
// 都来自这里。表单状态完全由 initial 派生：切换编辑对象由调用方用 `key` 强制 remount，
// 因此不需要 effect 回填，也不会出现「initial 异步到达但 state 已旧」的问题。

import { useMemo, useState, type FormEvent } from 'react';

import {
  validateProviderForm,
  type CloudProviderFormErrors,
  type CloudProviderFormValues,
} from '@/lib/cloudProviderValidation';
import type { CloudProviderRecord, CloudProviderUpsertInput } from '@/types/ipc';

/** 可编辑字段名（与 CloudProviderFormValues 的键一致）。 */
export type CloudProviderField = keyof CloudProviderFormValues;

/** useCloudProviderForm 的入参。 */
export interface UseCloudProviderFormOptions {
  /** 初始记录（编辑时传；null 视为新建） */
  initial: CloudProviderRecord | null;
  /** 校验通过后的提交回调 */
  onSubmit: (input: CloudProviderUpsertInput) => Promise<void> | void;
}

/** useCloudProviderForm 的对外出口。 */
export interface CloudProviderFormHandle {
  /** 是否为编辑模式（slug 不可改） */
  isEdit: boolean;
  /** 各字段当前值 */
  values: CloudProviderFormValues;
  /** 更新单个字段 */
  setField: (field: CloudProviderField, value: string) => void;
  /** 标记字段已触摸（失焦或首次提交时） */
  touch: (field: CloudProviderField) => void;
  /** 该字段此刻应展示的错误文案（未触摸或已通过则为 null） */
  errorFor: (field: CloudProviderField) => string | null;
  /** 表单级错误（校验未过或提交失败） */
  formError: string | null;
  /** 提交：校验不通过时只展示错误，不调用 onSubmit */
  handleSubmit: (evt: FormEvent<HTMLElement>) => Promise<void>;
}

/** 空串表示未填写，提交时转 null（后端字段可空）。 */
function toUpsertInput(values: CloudProviderFormValues): CloudProviderUpsertInput {
  return {
    provider_key: values.slug,
    name: values.name,
    remark: values.remark || null,
    website: values.website || null,
    base_url: values.baseUrl,
  };
}

/** 由初始记录派生表单初值（新建时全为空串）。 */
function initialValues(initial: CloudProviderRecord | null): CloudProviderFormValues {
  return {
    slug: initial?.provider_key ?? '',
    name: initial?.name ?? '',
    remark: initial?.remark ?? '',
    website: initial?.website ?? '',
    baseUrl: initial?.base_url ?? '',
  };
}

/**
 * 管理云提供商表单的状态与提交。
 *
 * Args:
 *   options: 初始记录与提交回调（见 UseCloudProviderFormOptions）
 *
 * Returns:
 *   字段值、触摸态、错误文案与提交处理（见 CloudProviderFormHandle）
 */
export function useCloudProviderForm({
  initial,
  onSubmit,
}: UseCloudProviderFormOptions): CloudProviderFormHandle {
  const isEdit = initial !== null;
  const [values, setValues] = useState<CloudProviderFormValues>(() => initialValues(initial));
  const [touched, setTouched] = useState<Partial<Record<CloudProviderField, boolean>>>({});
  const [formError, setFormError] = useState<string | null>(null);

  const errors: CloudProviderFormErrors = useMemo(
    () => validateProviderForm(values, { isEdit }),
    [values, isEdit],
  );
  const isValid = !Object.values(errors).some(Boolean);

  const setField = (field: CloudProviderField, value: string) => {
    setValues((prev) => ({ ...prev, [field]: value }));
  };

  const touch = (field: CloudProviderField) => {
    setTouched((prev) => ({ ...prev, [field]: true }));
  };

  const errorFor = (field: CloudProviderField): string | null =>
    touched[field] ? errors[field] : null;

  const handleSubmit = async (evt: FormEvent<HTMLElement>) => {
    evt.preventDefault();
    // 首次提交时全量标记 touched，让所有错误一次性展示
    setTouched({ slug: true, name: true, remark: true, website: true, baseUrl: true });
    if (!isValid) {
      setFormError('请修正下方表单错误');
      return;
    }
    setFormError(null);
    try {
      await onSubmit(toUpsertInput(values));
    } catch (err) {
      setFormError(err instanceof Error ? err.message : '保存提供商失败');
    }
  };

  return { isEdit, values, setField, touch, errorFor, formError, handleSubmit };
}
