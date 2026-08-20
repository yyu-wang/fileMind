// 分类预览树：按分类分组展示预览计划 + 待确认组 + 汇总统计（设计稿 §5.1）。
//
// 分组规则：`category_name` 非空 → 归入该分类组；为空 → 「待确认」组（黄标）。
// 冲突项（status=Conflict）单独标记，执行时由 Rust 跳过。

import { useMemo, useState } from 'react';

import { useClassifyStore, PENDING_NAME } from '@/stores/classifyStore';
import type { Category, ClassifyPlanItem, ClassifyPreview } from '@/types/ipc';
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
  // T6.12：手动分类数据源 + 动作（待确认/待人工确认文件用）
  const categories = useClassifyStore((s) => s.categories);
  const assignCategory = useClassifyStore((s) => s.assignCategory);
  const assignCategories = useClassifyStore((s) => s.assignCategories);
  // 批量勾选态（仅待确认组内未分类项可勾选，纯 UI 状态）
  const [selectedIds, setSelectedIds] = useState<string[]>([]);

  const pendingGroup = groups.find((g) => g.name === PENDING_NAME);
  const pendingItems = pendingGroup?.items ?? [];
  // 全选判断：当前勾选集是否覆盖该组全部未分类项
  const pendingFileIds = pendingItems.map((i) => i.file_id);
  const allSelected =
    pendingFileIds.length > 0 && pendingFileIds.every((id) => selectedIds.includes(id));

  const toggleSelect = (fileId: string) => {
    setSelectedIds((prev) =>
      prev.includes(fileId) ? prev.filter((id) => id !== fileId) : [...prev, fileId],
    );
  };

  const toggleSelectAll = () => {
    setSelectedIds(allSelected ? [] : pendingFileIds);
  };

  const handleAssign = (fileId: string, categoryName: string) => {
    const category = categories.find((c) => c.name === categoryName);
    if (category) {
      assignCategory(fileId, category);
    }
  };

  const handleBatchAssign = (categoryName: string) => {
    const category = categories.find((c) => c.name === categoryName);
    if (category && selectedIds.length > 0) {
      assignCategories(selectedIds, category);
      setSelectedIds([]);
    }
  };

  return (
    <div className="classify-preview">
      <div className="classify-preview__stats">
        <span>共 {preview.stats.total} 个文件</span>
        <span>规则 {preview.stats.by_rule}</span>
        <span>启发式 {preview.stats.by_heuristic}</span>
        <span>待确认 {preview.stats.pending}</span>
      </div>

      <div className="classify-preview__groups">
        {groups.map((group) => {
          const isPendingGroup = group.name === PENDING_NAME;
          return (
            <section
              key={group.name}
              className={
                isPendingGroup
                  ? 'classify-preview__group classify-preview__group--pending'
                  : 'classify-preview__group'
              }
            >
              <h3 className="classify-preview__group-head">
                {isPendingGroup && (
                  <label className="classify-preview__select-all" title="全选/取消全选">
                    <input
                      type="checkbox"
                      checked={allSelected}
                      onChange={toggleSelectAll}
                      aria-label="全选待确认文件"
                    />
                  </label>
                )}
                <span>{group.name}</span>
                <span className="classify-preview__group-count">{group.items.length}</span>
              </h3>

              {/* T6.12：批量指定分类横幅（勾选 ≥1 时显示） */}
              {isPendingGroup && selectedIds.length > 0 && (
                <div className="classify-preview__batch">
                  <span>已选 {selectedIds.length} 个文件</span>
                  <select
                    className="classify-preview__assign"
                    aria-label="为选中的文件批量指定分类"
                    value=""
                    onChange={(e) => handleBatchAssign(e.target.value)}
                  >
                    <option value="" disabled>
                      指定分类…
                    </option>
                    {categories.map((c: Category) => (
                      <option key={c.id} value={c.name}>
                        {c.name}
                      </option>
                    ))}
                  </select>
                  <button
                    type="button"
                    className="classify-preview__batch-cancel"
                    onClick={() => setSelectedIds([])}
                  >
                    取消
                  </button>
                </div>
              )}

              <ul className="classify-preview__list">
                {group.items.map((item) => (
                  <li key={item.file_id} className="classify-preview__item">
                    {/* T6.12：待确认/待人工确认文件可勾选（批量分类） */}
                    {isPendingGroup && (
                      <input
                        type="checkbox"
                        className="classify-preview__check"
                        checked={selectedIds.includes(item.file_id)}
                        onChange={() => toggleSelect(item.file_id)}
                        aria-label={`选择 ${item.file_name}`}
                      />
                    )}
                    <span className="classify-preview__name" title={item.original_path}>
                      {item.file_name}
                    </span>
                    <ClassifyRuleSourceTag source={item.rule_source} />
                    {conflictLabel(item) && (
                      <span className="classify-preview__conflict">{conflictLabel(item)}</span>
                    )}
                    {/* T6.12：单个文件手动指定分类 */}
                    {isPendingGroup && categories.length > 0 && (
                      <select
                        className="classify-preview__assign"
                        aria-label={`为 ${item.file_name} 指定分类`}
                        value=""
                        onChange={(e) => handleAssign(item.file_id, e.target.value)}
                      >
                        <option value="" disabled>
                          指定分类…
                        </option>
                        {categories.map((c) => (
                          <option key={c.id} value={c.name}>
                            {c.name}
                          </option>
                        ))}
                      </select>
                    )}
                  </li>
                ))}
              </ul>
            </section>
          );
        })}
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
