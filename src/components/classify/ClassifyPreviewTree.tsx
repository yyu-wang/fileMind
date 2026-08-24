// 分类预览树（对齐交互原型 §智能分类）：树形可折叠分组预览。
//
// 分组规则（对齐原型）：
//   - 分类组：`category_name` 非空且非冲突 → 按分类名分组（📁）
//   - 冲突组：`status=Conflict` → 独立「⚠️ 冲突」组（执行时按策略跳过）
//   - 待确认组：`category_name` 为空 → 「❓ 待确认」组（支持批量/单个手动指定分类）

import { useMemo, useState } from 'react';

import { useClassifyStore, PENDING_NAME } from '@/stores/classifyStore';
import { useFileStore } from '@/stores/fileStore';
import type { Category, ClassifyPlanItem, ClassifyPreview } from '@/types/ipc';
import { ClassifyRuleSourceTag } from './ClassifyRuleSourceTag';

interface ClassifyPreviewTreeProps {
  /** 分类预览结果 */
  preview: ClassifyPreview;
  /** 点击文件名打开预览抽屉（由父级传入对应文件的最小元信息） */
  onOpenPreview?: (item: ClassifyPlanItem) => void;
}

interface PreviewGroup {
  name: string;
  items: ClassifyPlanItem[];
}

/** 冲突组展示名。 */
const CONFLICT_NAME = '⚠️ 冲突';
/** 待确认组展示名（沿用 store 常量，保持统一）。 */
const PENDING_LABEL = `❓ ${PENDING_NAME}`;

/**
 * 按原型分组：分类组（排序）→ 冲突组 → 待确认组（恒排最后）。
 * 冲突项独立成组，不再混入对应分类组（与原型「⚠️ 冲突 (N)」一致）。
 */
function groupPreviewItems(items: ClassifyPlanItem[]): PreviewGroup[] {
  const conflict: ClassifyPlanItem[] = [];
  const pending: ClassifyPlanItem[] = [];
  const byCategory = new Map<string, ClassifyPlanItem[]>();

  for (const item of items) {
    if (item.status === 'Conflict') {
      conflict.push(item);
      continue;
    }
    if (item.category_name == null) {
      pending.push(item);
      continue;
    }
    const list = byCategory.get(item.category_name) ?? [];
    list.push(item);
    byCategory.set(item.category_name, list);
  }

  const groups: PreviewGroup[] = [...byCategory.entries()]
    .sort(([a], [b]) => a.localeCompare(b, 'zh-Hans-CN'))
    .map(([name, list]) => ({ name, items: list }));
  if (conflict.length > 0) {
    groups.push({ name: CONFLICT_NAME, items: conflict });
  }
  if (pending.length > 0) {
    groups.push({ name: PENDING_LABEL, items: pending });
  }
  return groups;
}

/** 从 target_path 提取相对扫描根的目标子目录（`/财务/`），供树子节点展示。 */
function targetSubdir(targetPath: string, scanPath: string | null): string {
  if (!scanPath) return '';
  if (!targetPath.startsWith(scanPath)) return '';
  const rest = targetPath.slice(scanPath.length).replace(/^\//, '');
  const dir = rest.includes('/') ? rest.slice(0, rest.lastIndexOf('/')) : '';
  return dir ? ` → /${dir}/` : '';
}

export function ClassifyPreviewTree({ preview, onOpenPreview }: ClassifyPreviewTreeProps) {
  const groups = useMemo(() => groupPreviewItems(preview.items), [preview.items]);
  const scanPath = useFileStore((s) => s.scanPath);
  // 手动分类数据源 + 动作（待确认组用）
  const categories = useClassifyStore((s) => s.categories);
  const assignCategory = useClassifyStore((s) => s.assignCategory);
  const assignCategories = useClassifyStore((s) => s.assignCategories);
  // 批量勾选态（仅待确认组内未分类项可勾选，纯 UI 状态）
  const [selectedIds, setSelectedIds] = useState<string[]>([]);
  // 树折叠态：组名 → 是否折叠（默认全展开）
  const [collapsed, setCollapsed] = useState<Record<string, boolean>>({});

  const pendingGroup = groups.find((g) => g.name === PENDING_LABEL);
  const pendingItems = pendingGroup?.items ?? [];
  const pendingFileIds = pendingItems.map((i) => i.file_id);
  const allSelected =
    pendingFileIds.length > 0 && pendingFileIds.every((id) => selectedIds.includes(id));

  const toggleGroup = (name: string) => {
    setCollapsed((prev) => ({ ...prev, [name]: !prev[name] }));
  };

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
      <div className="classify-tree-header">
        <h3>分类预览</h3>
        <span className="count">
          {preview.stats.by_rule + preview.stats.by_heuristic} 个已分类 · {preview.stats.pending}{' '}
          个待确认
        </span>
      </div>

      <div className="classify-tree">
        {groups.map((group) => {
          const isPendingGroup = group.name === PENDING_LABEL;
          const isConflictGroup = group.name === CONFLICT_NAME;
          const isCollapsed = collapsed[group.name] === true;
          return (
            <div
              key={group.name}
              className={`tree-panel${isPendingGroup ? ' confirm' : ''}${isConflictGroup ? ' conflict' : ''}`}
            >
              <div
                className="tree-node parent"
                onClick={() => toggleGroup(group.name)}
                role="button"
                aria-expanded={!isCollapsed}
              >
                <span className={`arrow${isCollapsed ? '' : ' expanded'}`} aria-hidden />
                <span>{group.name}</span>
                {isPendingGroup && (
                  <label
                    className="classify-preview__select-all"
                    onClick={(e) => e.stopPropagation()}
                  >
                    <input
                      type="checkbox"
                      checked={allSelected}
                      onChange={toggleSelectAll}
                      aria-label="全选待确认文件"
                    />
                  </label>
                )}
                <span className="node-count">
                  {group.items.length} 文件{isConflictGroup ? ' · 需处理' : ''}
                </span>
              </div>

              {!isCollapsed && (
                <div className="tree-children">
                  {/* 批量指定分类横幅（待确认组勾选 ≥1 时显示） */}
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

                  {group.items.map((item) => (
                    <div key={item.file_id} className="tree-node child">
                      {/* 待确认组文件可勾选（批量分类） */}
                      {isPendingGroup && (
                        <input
                          type="checkbox"
                          className="classify-preview__check"
                          checked={selectedIds.includes(item.file_id)}
                          onChange={() => toggleSelect(item.file_id)}
                          aria-label={`选择 ${item.file_name}`}
                        />
                      )}
                      {/* 提供 onOpenPreview 时文件名可点击，打开预览抽屉（对齐文件页） */}
                      {onOpenPreview ? (
                        <button
                          type="button"
                          className="tree-node__name tree-node__name--preview"
                          title={item.original_path}
                          onClick={() => onOpenPreview(item)}
                        >
                          {item.file_name}
                        </button>
                      ) : (
                        <span className="tree-node__name" title={item.original_path}>
                          {item.file_name}
                        </span>
                      )}
                      <ClassifyRuleSourceTag source={item.rule_source} />
                      {isConflictGroup && (
                        <span className="tree-node__conflict">目标已存在（跳过）</span>
                      )}
                      {!isPendingGroup && !isConflictGroup && (
                        <span className="tree-node__dir">
                          {targetSubdir(item.target_path, scanPath)}
                        </span>
                      )}
                      {/* 待确认文件单个指定分类（对齐原型「确认/改分类」） */}
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
                    </div>
                  ))}
                </div>
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
}
