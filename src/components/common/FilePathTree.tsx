// 文件路径树：展示文件所在的目录层级，高亮当前文件节点。
//
// 对齐原型中「所在位置」的 ASCII 树形风格：
//   📁 Documents/
//   ├── 📁 市场/
//   │   ├── 📁 竞品/
//   │   └── 📁 报告/
//   └── 📁 财务/
//         └── 📄 年报.pdf  ← 当前文件高亮

import { useMemo } from 'react';

interface FilePathTreeProps {
  /** 文件绝对路径 */
  path: string;
  /** 文件名（单独传入以避免路径解析错误） */
  fileName: string;
}

interface PathNode {
  name: string;
  isLast: boolean;
  isLeaf: boolean;
  isTarget: boolean;
  depth: number;
  prefix: string[];
}

/**
 * 按路径段顺序生成树形节点。
 *
 * 因为单个文件路径的每一级节点都是父级的唯一孩子，
 * 前缀分支仅由「该层是否是链的最后一环」来决定：
 * 紧邻父级用 ├── / └──，更外层用 4 空格占位。
 */
function buildSimpleNodes(fullPath: string, leafName: string): PathNode[] {
  const normalized = fullPath.replace(/\\/g, '/');
  const segments = normalized.split('/').filter((s) => s.length > 0);
  const dirSegments = segments.slice(0, -1);
  const nodes: PathNode[] = [];
  const total = dirSegments.length + 1;

  for (let i = 0; i < total; i += 1) {
    const name = i < dirSegments.length ? dirSegments[i] : leafName;
    const isLeaf = i === total - 1;

    const prefix: string[] = [];
    for (let d = 0; d < i; d += 1) {
      if (d === i - 1) {
        prefix.push(isLeaf ? '└── ' : '├── ');
      } else {
        prefix.push('    ');
      }
    }

    nodes.push({
      name,
      isLast: i === total - 1,
      isLeaf,
      isTarget: isLeaf,
      depth: i,
      prefix,
    });
  }

  return nodes;
}

export function FilePathTree({ path, fileName }: FilePathTreeProps) {
  const nodes = useMemo(() => buildSimpleNodes(path, fileName), [path, fileName]);

  return (
    <div className="file-path-tree" role="tree" aria-label="文件所在位置">
      <div className="file-path-tree__title">📍 所在位置</div>
      <div className="file-path-tree__body">
        {nodes.map((node, idx) => (
          <div
            // biome-ignore lint/suspicious/noArrayIndexKey: 顺序稳定
            key={idx}
            className={`file-path-tree__node ${node.isTarget ? 'is-target' : ''} ${node.isLeaf ? 'is-leaf' : 'is-dir'}`}
            role="treeitem"
            aria-selected={node.isTarget}
          >
            <span className="file-path-tree__prefix font-mono">
              {node.prefix.map((seg, segIdx) => (
                // biome-ignore lint/suspicious/noArrayIndexKey: 顺序稳定
                <span key={segIdx}>{seg}</span>
              ))}
            </span>
            <span className="file-path-tree__icon">{node.isLeaf ? '📄' : '📁'}</span>
            <span
              className={`file-path-tree__name ${node.isTarget ? 'is-target' : ''}`}
              title={node.name}
            >
              {node.name}
              {!node.isLeaf && '/'}
            </span>
            {node.isTarget && <span className="file-path-tree__badge">当前文件</span>}
          </div>
        ))}
      </div>
    </div>
  );
}
