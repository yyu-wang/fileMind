// 文件预览内容渲染：按类型分发到 文本 / HTML / 图片 / PDF / 不支持。
//
// 与 FilePreviewDrawer 分离的原因：抽屉负责「遮罩 / 标题 / 目录树 / meta」外壳，
// 内容渲染自成一组（含 PDF 分页与 HTML 沙箱两个有状态子组件）；拆开后两者都在
// complexity 规则的组件行数阈值内。

import { useState } from 'react';
import { Document, Page, pdfjs } from 'react-pdf';

import type { FilePreview } from '@/types/ipc';

// PDF worker 用同源 URL（CSP script-src 'self' 禁 blob:）——紧邻 Document/Page 使用点
pdfjs.GlobalWorkerOptions.workerSrc = new URL(
  'pdfjs-dist/build/pdf.worker.min.mjs',
  import.meta.url,
).toString();

/** 从文件名取出扩展名（小写，不含点）。 */
export function fileExt(fileName: string): string | null {
  const idx = fileName.lastIndexOf('.');
  if (idx < 0 || idx === fileName.length - 1) return null;
  return fileName.slice(idx + 1).toLowerCase();
}

interface PreviewContentProps {
  preview: FilePreview;
  pageNumber: number;
  numPages: number | null;
  onDocumentLoad: (numPages: number) => void;
  onDocumentError: (message: string) => void;
  onPageChange: (page: number) => void;
}

interface TextPreviewProps {
  text: string | null;
  fileName: string;
  truncated: boolean;
}

/**
 * 文本 / HTML 预览。独立组件定义在 switch 外部，避免 React Hooks 出现在
 * switch case 分支里（违反 rules-of-hooks 顺序不变性 + 触发 setState-in-effect）。
 */
function TextPreview({ text, fileName, truncated }: TextPreviewProps) {
  const ext = fileExt(fileName);
  const isHtml = ext === 'html' || ext === 'htm';
  // 用 text+fileName 作为 key：当文件/内容变更时 React 自动重建该子组件，
  // useState 自动回到初始 true，无需用 useEffect + setState 去"重置 loading"，
  // 也就避免了 react-hooks/set-state-in-effect lint 报错。
  const loadKey = `${fileName}::${text?.length ?? 0}`;
  return (
    <div className="files-preview__text" key={loadKey}>
      {truncated && <div className="files-preview__hint">内容过长，仅预览前 50MB</div>}
      {isHtml ? <HtmlFrame text={text} fileName={fileName} /> : <pre>{text ?? ''}</pre>}
    </div>
  );
}

interface HtmlFrameProps {
  text: string | null;
  fileName: string;
}

/**
 * HTML iframe 沙箱渲染。组件的挂载/卸载由父层 key={loadKey} 控制，
 * 每次切文件/改内容都重建：useState 初始 true，onload 后 false，
 * 不需要 effect 去 reset loading。
 */
function HtmlFrame({ text, fileName }: HtmlFrameProps) {
  const [htmlLoading, setHtmlLoading] = useState(true);
  return (
    <div className="files-preview__html-wrap">
      {htmlLoading && (
        <div className="files-preview__html-loading" role="status" aria-live="polite">
          <span className="files-preview__spinner" aria-hidden />
          <span>HTML 渲染中…</span>
        </div>
      )}
      <iframe
        title={fileName}
        className="files-preview__html"
        sandbox=""
        srcDoc={text ?? ''}
        referrerPolicy="no-referrer"
        onLoad={() => setHtmlLoading(false)}
      />
    </div>
  );
}

export function PreviewContent({
  preview,
  pageNumber,
  numPages,
  onDocumentLoad,
  onDocumentError,
  onPageChange,
}: PreviewContentProps) {
  switch (preview.kind) {
    case 'Text':
      return (
        <TextPreview
          text={preview.text}
          fileName={preview.file_name}
          truncated={preview.truncated}
        />
      );
    case 'Image':
      return preview.data_url ? (
        <div className="files-preview__image">
          <img src={preview.data_url} alt={preview.file_name} />
        </div>
      ) : null;
    case 'Pdf':
      return preview.data_url ? (
        <div className="files-preview__pdf">
          <Document
            file={preview.data_url}
            onLoadSuccess={({ numPages: pages }) => onDocumentLoad(pages)}
            onLoadError={(error) => onDocumentError(`PDF 加载失败（${error.message}）`)}
          >
            <Page pageNumber={pageNumber} />
          </Document>
          <div className="files-preview__pager">
            <button
              type="button"
              className="files-preview__pagebtn"
              aria-label="上一页"
              disabled={pageNumber <= 1}
              onClick={() => onPageChange(pageNumber - 1)}
            >
              ‹
            </button>
            <span className="files-preview__pageinfo">
              {pageNumber} / {numPages ?? '…'}
            </span>
            <button
              type="button"
              className="files-preview__pagebtn"
              aria-label="下一页"
              disabled={numPages != null && pageNumber >= numPages}
              onClick={() => onPageChange(pageNumber + 1)}
            >
              ›
            </button>
          </div>
        </div>
      ) : null;
    case 'Unsupported':
      return <div className="files-preview__unsupported">暂不支持预览该文件类型</div>;
  }
}
