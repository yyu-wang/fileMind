// 生成分类预览的动作：重入守卫 + 过期响应丢弃 + 预览结果落库。
//
// 与 store 分离的原因：这段时序逻辑（FE-M9 的序号守卫）自成一整块，留在 create 回调里
// 会把回调推过函数行数阈值；抽成模块后 store 只注入 set/get，公开符号保持不变。
//
// 预览序号随之落在本模块：reset 通过 `invalidatePreview()` 作废在途预览，
// 与执行令牌（runActions 的 `invalidateExecution()`）语义一致。

import { fileIpc } from '@/lib/ipc';
import { ipcErrorMessage } from '@/lib/ipcError';
import { ClassifyStatus } from '@/types/models';
import { useFileStore } from '../fileStore';
import type { ClassifyState } from './types';

/** 预览动作所需的最小依赖面（与 lifecycle 的注入方式一致，避免子模块反向依赖 store）。 */
export interface PreviewDeps {
  /** 写入状态（zustand 的 set） */
  set: (patch: Partial<ClassifyState>) => void;
  /** 读当前状态（zustand 的 get） */
  get: () => ClassifyState;
}

// FE-M9：预览请求序号——生成中的慢响应被后续请求/取消作废，
// 到达后序号失配直接丢弃，防止旧预览覆盖新状态。
let previewSeq = 0;

/** 作废在途预览（reset 调用；慢响应到达后因序号失配被丢弃）。 */
export function invalidatePreview(): void {
  previewSeq += 1;
}

/**
 * 生成预览动作（供 store 展开进 create 的返回对象）。
 *
 * Args:
 *   deps: store 注入的依赖面
 *
 * Returns:
 *   生成分类预览动作
 */
export function createPreviewActions(deps: PreviewDeps): Pick<ClassifyState, 'generatePreview'> {
  const { set, get } = deps;
  return {
    generatePreview: async (fileIds) => {
      const scanPath = useFileStore.getState().scanPath;
      if (!scanPath) {
        set({ status: ClassifyStatus.Idle, error: '请先在文件页选择要整理的目录' });
        return;
      }
      // FE-M9：重入守卫——StrictMode 双挂载/用户快速连点时，进行中的预览
      // 直接忽略后续调用（旧实现并发跑两个 IPC，晚回者覆盖早回者）
      if (get().status === ClassifyStatus.Previewing) return;
      const seq = ++previewSeq;
      set({ status: ClassifyStatus.Previewing, error: null, preview: null, execSummary: null });
      try {
        const result = await fileIpc.classifyPreview(fileIds, scanPath);
        // FE-M9：期间有新请求发起或 reset 被调用，本次响应已过期，丢弃
        if (seq !== previewSeq) return;
        if (result.status === 'ok') {
          const pendingIds = result.data.items
            .filter((item) => item.category_name == null)
            .map((item) => item.file_id);
          set({ status: ClassifyStatus.Idle, preview: result.data, pendingIds });
          // T6.12：加载分类列表供手动分类下拉使用（幂等，失败不影响预览）
          void get().loadCategories();
        } else {
          set({ status: ClassifyStatus.Idle, error: result.error });
        }
      } catch (err) {
        // 必须复位：status 卡在 Previewing 会让上面的重入守卫永久拒绝后续预览，
        // 分类页彻底不可用（按钮一直显示生成中）
        if (seq === previewSeq) set({ status: ClassifyStatus.Idle, error: ipcErrorMessage(err) });
      }
    },
  };
}
