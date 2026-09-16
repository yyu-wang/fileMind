// 引用跳转：点击回答里的引用标签 → 定位文件 → 打开右侧预览抽屉。
//
// 这段流程原先内联在 ChatPage 的 handleCitationClick 里（83 行、圈复杂度 19）：五条退出
// 路径各自判断、各自清理同一个持久 toast，堆在一起才超限。抽成 hook 后每条路径只管自己的
// 判断，toast 清理统一收在 finally。
//
// 定位策略：本地 fileStore 列表优先（findFileByName 四级内存匹配 <1ms），未命中再走 IPC
// 模糊搜索；列表为空时先补全一次再匹配——ChatPage 可能是进入 App 的首个路由页。

import { useCallback, useEffect, useRef, useState } from 'react';

import { useToastStore } from '@/components/ui/Toast';
import { findFileByName, hasUsableFileName } from '@/lib/citation';
import { useFileStore } from '@/stores/fileStore';
import type { FileInfo } from '@/types/ipc';
import type { ChatCitation } from '@/types/models';

/** 预览目标：引用命中的文件与需要定位的页码。 */
export interface CitationPreviewTarget {
  file: FileInfo;
  /** 引用页码（抽屉打开后定位到该页） */
  initialPage: number;
}

/** useCitationPreview 的对外出口。 */
export interface CitationPreviewHandle {
  /** 打开引用并定位预览（void 包装，可直接作为消息列表的回调） */
  openCitation: (citation: ChatCitation) => void;
  /** 关闭预览抽屉 */
  close: () => void;
  /** 抽屉挂载 key：每次打开自增，强制 react-pdf 重挂载以清掉旧的缩放/翻页状态 */
  mountKey: number;
  /** 当前预览目标，null 表示抽屉关闭 */
  target: CitationPreviewTarget | null;
}

/** 补全文件列表：失败返回 null，提示文案由调用方给出。 */
async function loadFilesForCitation(loadAllFiles: () => Promise<void>): Promise<FileInfo[] | null> {
  try {
    await loadAllFiles();
  } catch {
    return null;
  }
  return useFileStore.getState().files;
}

/** 管理引用跳转的状态与过程提示，返回预览目标、挂载 key 与开关回调。 */
export function useCitationPreview(): CitationPreviewHandle {
  const loadAllFiles = useFileStore((s) => s.loadAllFiles);
  const [target, setTarget] = useState<CitationPreviewTarget | null>(null);
  const [mountKey, setMountKey] = useState(0);

  // FE-m14：await 后 setState 的卸载守卫，防卸载后 setState warning。必须在 setup 里显式置
  // true：StrictMode（dev 双跑 setup→cleanup→setup）与 HMR Fast Refresh 都会先执行 cleanup，
  // 若只在初始化 useRef(true) 里赋值，ref 会永久停在 false，点击引用将静默 return。
  const isMountedRef = useRef(true);
  useEffect(() => {
    isMountedRef.current = true;
    return () => {
      isMountedRef.current = false;
    };
  }, []);

  const runOpenCitation = useCallback(
    async (citation: ChatCitation) => {
      // 最外层兜底：任何 throw 都转为错误 toast，避免 unhandled rejection 让用户以为「点了没反应」
      const { show, remove } = useToastStore.getState();
      let loadingToastId: string | null = null;
      const clearLoadingToast = () => {
        if (loadingToastId !== null) {
          remove(loadingToastId);
          loadingToastId = null;
        }
      };

      try {
        if (!hasUsableFileName(citation)) {
          show({ message: '引用文件名为空，请检查回答内容格式', variant: 'error' });
          return;
        }

        // ① 本地列表为空才需要「等待中」提示：接下来要走补全 + 兜底搜索的慢路径
        let files = useFileStore.getState().files;
        if (files.length === 0) {
          loadingToastId = show({
            message: `正在定位「${citation.fileName}」…`,
            variant: 'info',
            duration: 0,
          });
          const loaded = await loadFilesForCitation(loadAllFiles);
          if (loaded === null) {
            show({ message: '文件列表加载失败，稍后重试', variant: 'error' });
            return;
          }
          if (!isMountedRef.current) return;
          files = loaded;
        }

        // ② 四级内存匹配 + IPC 兜底；fastPath=true 表示内存命中（<1ms）
        const found = await findFileByName(citation.fileName, files);
        if (!isMountedRef.current) return;
        if (!found) {
          show({
            message: `未找到引用文件「${citation.fileName}」，请先在文件管理中扫描该目录`,
            variant: 'warn',
            duration: 4500,
          });
          return;
        }

        // ③ 只有走 IPC 的慢路径才补「正在加载预览」，内存命中不必打扰用户
        if (!found.fastPath && loadingToastId === null) {
          loadingToastId = show({
            message: `正在加载「${citation.fileName}」预览…`,
            variant: 'info',
            duration: 0,
          });
        }

        // ④ mountKey 自增（强制卸旧实例、清 PDF 缓存）+ 设置目标（React 批处理为一次渲染）
        setMountKey((k) => k + 1);
        setTarget({ file: found.file, initialPage: citation.page });
      } catch (err) {
        show({
          message: `打开预览失败：${err instanceof Error ? err.message : String(err)}`,
          variant: 'error',
          duration: 5000,
        });
      } finally {
        // 统一收尾：旧版手写清理唯独漏了 catch 路径，而 loading toast 的 duration 为 0
        // （不自动关闭），异常时会在页面上永久残留。
        clearLoadingToast();
      }
    },
    [loadAllFiles],
  );

  /** void 包装：调用方不关心返回的 Promise（暴露 Promise 会招来未处理拒绝） */
  const openCitation = useCallback(
    (citation: ChatCitation) => {
      void runOpenCitation(citation);
    },
    [runOpenCitation],
  );

  const close = useCallback(() => setTarget(null), []);

  return { openCitation, close, mountKey, target };
}
