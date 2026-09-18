// 「指定分类」下拉：受控 value 恒为空，选中即回调一次（由父级决定是单文件还是批量）。
//
// 批量横幅与单文件行原先各写了一份 option 列表，抽成本组件后两处共用同一份渲染。

import type { Category } from '@/types/ipc';

interface ClassifyCategorySelectProps {
  /** 无障碍标签（批量横幅与单文件行文案不同） */
  label: string;
  /** 分类下拉数据源 */
  categories: Category[];
  /** 选中的分类名 */
  onChange: (categoryName: string) => void;
  /** 样式类（批量横幅与单文件行共用同一类名） */
  className: string;
}

export function ClassifyCategorySelect({
  label,
  categories,
  onChange,
  className,
}: ClassifyCategorySelectProps) {
  return (
    <select
      className={className}
      aria-label={label}
      value=""
      onChange={(e) => onChange(e.target.value)}
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
  );
}
