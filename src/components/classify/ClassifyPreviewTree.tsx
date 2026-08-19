// 分类预览树：按分类分组展示预览计划 + 待确认组 + 汇总统计（设计稿 §5.1）。
//
// 分组规则：`category_name` 非空 → 归入该分类组；为空 → 「待确认」组（黄标）。
// 冲突项（status=Conflict）单独标记，执行时由 Rust 跳过。

import { useMemo } from 'react';

import { PENDING_NAME } from '@/stores/classifyStore';
import type { ClassifyPlanItem, ClassifyPreview } from '@/types/ipc';
import { ClassifyRuleSourceTag } from './ClassifyRuleSourceTag';

interface ClassifyPreviewTreeProps {
  /** 分类预览结果 */
  preview: ClassifyPreview;
  /** 点击「开始执行」 */
  onExecute: () => void;
  /** 点击「重新选择」（回到分类页初始态） */
  onReset: () => void;
}

interface PreviewGroup {
  name: string;
  items: ClassifyPlanItem[];
}

/** 将预览项按分类名分组；`null` 归类为待确认组（恒排最后）。 */
function groupByCategory(items: ClassifyPlanItem[]): PreviewGroup[] {
  const groups = new Map<string, PreviewGroup>();
  for (const item of items) {
    const name = item.category_name ?? PENDING_NAME;
    const existing = groups.get(name);
    if (existing) {
      existing.items.push(item);
    } else {
      groups.set(name, { name, items: [item] });
    }
  }
  // 待确认组固定展示在最后，其余按名称排序
  return [...groups.entries()]
    .sort(([a], [b]) => {
      if (a === PENDING_NAME) return 1;
      if (b === PENDING_NAME) return -1;
      return a.localeCompare(b, 'zh-Hans-CN');
    })
    .map(([, group]) => group);
}

/** 分组标题下的冲突标记文案。 */
function conflictLabel(item: ClassifyPlanItem): string {
  return item.status === 'Conflict' ? '目标已存在（跳过）' : '';
}

export function ClassifyPreviewTree({ preview, onExecute, onReset }: ClassifyPreviewTreeProps) {
  const groups = useMemo(() => groupByCategory(preview.items), [preview.items]);

  return (
    <div className="classify-preview">
      <div className="classify-preview__stats">
        <span>共 {preview.stats.total} 个文件</span>
        <span>规则 {preview.stats.by_rule}</span>
        <span>启发式 {preview.stats.by_heuristic}</span>
        <span>待确认 {preview.stats.pending}</span>
      </div>

      <div className="classify-preview__groups">
        {groups.map((group) => (
          <section
            key={group.name}
            className={
              group.name === PENDING_NAME
                ? 'classify-preview__group classify-preview__group--pending'
                : 'classify-preview__group'
            }
          >
            <h3 className="classify-preview__group-head">
              <span>{group.name}</span>
              <span className="classify-preview__group-count">{group.items.length}</span>
            </h3>
            <ul className="classify-preview__list">
              {group.items.map((item) => (
                <li key={item.file_id} className="classify-preview__item">
                  <span className="classify-preview__name" title={item.original_path}>
                    {item.file_name}
                  </span>
                  <ClassifyRuleSourceTag source={item.rule_source} />
                  {conflictLabel(item) && (
                    <span className="classify-preview__conflict">{conflictLabel(item)}</span>
                  )}
                </li>
              ))}
            </ul>
          </section>
        ))}
      </div>

      <div className="classify-preview__actions">
        <button type="button" className="btn btn--primary" onClick={onExecute}>
          开始执行
        </button>
        <button type="button" className="btn btn--ghost" onClick={onReset}>
          重新选择
        </button>
      </div>
    </div>
  );
}
