// 分类统计面板（对齐交互原型 §智能分类 右侧 stats-panel）。
//
// 三张卡片：
//   1. 分类统计：规则/启发式/LLM/待确认 数量 + 占比进度条
//   2. 安全提示：撤销恢复说明
//   3. 目标结构：本次分类将生成的目录树

import { useMemo } from 'react';

import { useFileStore } from '@/stores/fileStore';
import type { ClassifyPreview } from '@/types/ipc';

interface ClassifyStatsPanelProps {
  /** 分类预览结果（含 stats 与逐项数据） */
  preview: ClassifyPreview;
}

interface StatRow {
  label: string;
  count: number;
  color: string;
}

/** 分类统计行 + 进度条。 */
function StatBar({ label, count, total, color }: StatRow & { total: number }) {
  const pct = total > 0 ? Math.round((count / total) * 100) : 0;
  return (
    <>
      <div className="stat-row">
        <span className="label">{label}</span>
        <span className="value" style={{ color }}>
          {count} ({pct}%)
        </span>
      </div>
      <div className="stat-bar">
        <div className="stat-bar-fill" style={{ width: `${pct}%`, background: color }} />
      </div>
    </>
  );
}

export function ClassifyStatsPanel({ preview }: ClassifyStatsPanelProps) {
  const scanPath = useFileStore((s) => s.scanPath);
  // FE-M10：total 同样从 items 派生，与五行计数口径一致（stats.total 是预览
  // 时点快照，手动分配增删项后不再相等）
  const total = preview.items.length;

  // FE-M10：统计口径统一为「从 items 派生」——后端 stats 是预览时点快照，
  // 手动分配（rule_source='manual'）不落在 by_rule/by_heuristic/llm 任何一行，
  // 混用会导致四行加总 < total。全部行改为对 items 现算，加总恒等于 total。
  const derived = useMemo(() => {
    let byRule = 0;
    let byHeuristic = 0;
    let llm = 0;
    let manual = 0;
    let pending = 0;
    for (const item of preview.items) {
      if (item.category_name == null) {
        pending += 1;
      } else if (item.rule_source === 'manual') {
        manual += 1;
      } else if (item.rule_source === 'llm') {
        llm += 1;
      } else if (item.rule_source.startsWith('rule:')) {
        byRule += 1;
      } else {
        // heuristic 及其他来源（后端 rule_source: rule:<名>/heuristic/pending）
        byHeuristic += 1;
      }
    }
    return { byRule, byHeuristic, llm, manual, pending };
  }, [preview.items]);

  const rows: (StatRow & { key: string })[] = [
    { key: 'rule', label: '规则命中', count: derived.byRule, color: 'var(--success)' },
    {
      key: 'heuristic',
      label: '按类型识别',
      count: derived.byHeuristic,
      color: 'var(--accent2)',
    },
    { key: 'llm', label: 'AI 判断', count: derived.llm, color: 'var(--accent)' },
    { key: 'manual', label: '手动指定', count: derived.manual, color: 'var(--accent-light)' },
    { key: 'pending', label: '待确认', count: derived.pending, color: 'var(--warn)' },
  ];

  // 目标结构：分类名去重（排除冲突项与未分类）
  const categories = useMemo(() => {
    const set = new Set<string>();
    for (const item of preview.items) {
      if (item.category_name != null && item.status !== 'Conflict') {
        set.add(item.category_name);
      }
    }
    return Array.from(set).sort((a, b) => a.localeCompare(b, 'zh-Hans-CN'));
  }, [preview.items]);

  // FE-m3：兼容 Windows 反斜杠路径分隔符；输出根取收纳目录名（`<扫描根名>_已分类`），
  // 后端 `preview.output_root` 保证与目标拼接同源，避免本地再算造成不一致。
  const rootName = preview.output_root
    ? preview.output_root.split(/[\\/]/).filter(Boolean).pop()
    : scanPath
      ? scanPath.split(/[\\/]/).filter(Boolean).pop()
      : '文件库';

  return (
    <div className="stats-panel">
      <div className="stats-card">
        <h4>分类统计</h4>
        {rows.map((row) => (
          <div key={row.key} className="stats-card__row">
            <StatBar {...row} total={total} />
          </div>
        ))}
      </div>

      <div className="stats-card stats-card--tip">
        <div className="stats-card__tip-title">安全提示</div>
        <div className="stats-card__tip-text">
          执行后所有操作可通过「撤销」恢复。删除走系统回收站。
        </div>
      </div>

      <div className="stats-card">
        <h4>目标结构</h4>
        <div className="stats-tree">
          <div className="stats-tree__root">📂 {rootName}/</div>
          {categories.map((name) => (
            <div key={name} className="stats-tree__child">
              ├── 📁 {name}/
            </div>
          ))}
          {categories.length === 0 && <div className="stats-tree__child">├── （无可分类文件）</div>}
        </div>
      </div>
    </div>
  );
}
