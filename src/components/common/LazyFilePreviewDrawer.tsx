// FilePreviewDrawer 的懒加载包装。
//
// react-pdf + pdfjs 体积大，却只在打开预览抽屉时才需要；经此 React.lazy
// 边界把它们拆进独立异步 chunk，首屏主包不再加载。三个使用方
// （Files / Classify / Chat 页）统一经本包装引用。
//
// fallback 渲染 null：抽屉是 fixed 覆盖层，chunk 为本地资源毫秒级加载，
// 完成前短暂空白可接受，且不会引起布局跳动。

import { lazy, Suspense, type ComponentProps } from 'react';

const FilePreviewDrawer = lazy(() =>
  import('./FilePreviewDrawer').then((m) => ({ default: m.FilePreviewDrawer })),
);

type FilePreviewDrawerProps = ComponentProps<typeof FilePreviewDrawer>;

/** 与 FilePreviewDrawer 同 props 的懒加载版本（类型由 lazy 自动推断）。 */
export function LazyFilePreviewDrawer(props: FilePreviewDrawerProps) {
  return (
    <Suspense fallback={null}>
      <FilePreviewDrawer {...props} />
    </Suspense>
  );
}
