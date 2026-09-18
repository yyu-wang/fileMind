// 模型供给动作：Embedding / Rerank / 本地 GGUF 的下载与离线包导入。
//
// 与 ollama.ts 分离的原因：Ollama 探测是「环境状态查询」（TTL 节流、结果回写三个
// modelOptions 字段），模型下载是「长任务 + 进度轮询 + 幂等导入」，只共享 store 字段。
// 真实下载 / 落盘在 Sidecar（见 services/model_download_service.py）；离线导入是下载
// 之外的另一条通路：内网机器直接拷贝另一台机器的模型文件。

import { fileIpc } from '@/lib/ipc';
import type { SettingsGet, SettingsSet, SettingsState } from './types';

/**
 * 导入离线模型包并回写结果。
 *
 * Args:
 *   deps: store 注入的 set / get
 *   path: 用户选定的包路径（zip 或模型目录），后端会再过一次路径安全校验
 */
async function importModelPackageAction(
  deps: { set: SettingsSet; get: SettingsGet },
  path: string,
): Promise<void> {
  const { set, get } = deps;
  // 耗时数十秒：importingPackage 用于禁用入口，避免重复提交同一份拷贝
  set({ importingPackage: true, importResult: null, importError: null });
  try {
    const result = await fileIpc.importModelPackage(path);
    if (result.status === 'ok') {
      set({ importResult: result.data, importError: null });
      // 导入的模型可能刚转为就绪 → 绕过节流刷新探测，让「已就绪 / 未下载」徽标立刻更新
      await get().probeOllama(true);
    } else {
      // 错误码前缀与缺失文件明细由 Rust / Sidecar 拼好，原样透出给用户
      set({ importError: result.error });
    }
  } catch (e) {
    set({ importError: e instanceof Error ? e.message : String(e) });
  } finally {
    // 必须复位：导入失败时若不复位，按钮会永久卡在「导入中…」
    set({ importingPackage: false });
  }
}

/**
 * 把某个键的最新值并入映射（返回新对象：zustand 靠引用变化通知订阅者）。
 *
 * `value` 传 null 表示清掉该键：留着上一轮的 downloading / ready 会让卡片显示假状态
 * （明明没在下载，却一直转圈）。
 *
 * Args:
 *   map: 现有的键值映射（下载状态或启动错误）
 *   key: 目标模型标识（映射键）
 *   value: 新值；null 表示移除该键
 *
 * Returns:
 *   更新后的映射（不改动入参对象）
 */
function withEntry<T>(map: Record<string, T>, key: string, value: T | null): Record<string, T> {
  if (value === null) {
    return Object.fromEntries(Object.entries(map).filter(([name]) => name !== key));
  }
  return { ...map, [key]: value };
}

/**
 * 下载启动失败：记录该模型的错误并清掉它的下载状态条目。
 *
 * 独立成函数的原因：成功与异常两条路径（`status !== 'ok'` 与抛异常）共用这段回写。
 */
function failModelDownload(
  deps: { set: SettingsSet; get: SettingsGet },
  modelName: string,
  error: string,
): void {
  const { set, get } = deps;
  set({
    installErrors: withEntry(get().installErrors, modelName, error),
    downloadingModel: null,
    modelDownloads: withEntry(get().modelDownloads, modelName, null),
  });
}

/** 启动（或重试）某模型的下载并回写起始状态（进度由设置页轮询，本调用立即返回）。 */
async function startModelDownloadAction(
  deps: { set: SettingsSet; get: SettingsGet },
  modelName: string,
): Promise<void> {
  const { set, get } = deps;
  // Sidecar 侧下载是后台任务：本调用立即返回起始状态，进度靠轮询。
  // 只清本次模型的旧错误：清空整张表会让其它模型卡片刚显示的失败原因闪没
  set({
    downloadingModel: modelName,
    installErrors: withEntry(get().installErrors, modelName, null),
  });
  try {
    const result = await fileIpc.startModelDownload(modelName);
    if (result.status === 'ok') {
      set({ modelDownloads: withEntry(get().modelDownloads, modelName, result.data) });
    } else {
      failModelDownload(deps, modelName, result.error);
    }
  } catch (e) {
    failModelDownload(deps, modelName, e instanceof Error ? e.message : String(e));
  }
}

/**
 * 生成模型下载 / 离线导入动作（供 store 展开进 create 的返回对象）。
 *
 * Args:
 *   deps: store 注入的 set / get
 */
export function createModelDownloadActions(deps: {
  set: SettingsSet;
  get: SettingsGet;
}): Pick<SettingsState, 'startModelDownload' | 'refreshModelDownload' | 'importModelPackage'> {
  const { set, get } = deps;
  return {
    startModelDownload: (modelName) => startModelDownloadAction(deps, modelName),

    refreshModelDownload: async (modelName) => {
      // 轮询路径：查询失败静默保持上次状态（Sidecar 可能正在重启；下载失败会由
      // status=failed 携带 error，无需在这里额外标记）
      try {
        const result = await fileIpc.modelDownloadStatus(modelName);
        if (result.status !== 'ok') {
          return;
        }
        const status = result.data;
        const downloads = get().modelDownloads;
        // 只看该模型自己的旧状态：其它模型（如本地 GGUF）的状态变化不该影响本模型的判定
        const wasReady = downloads[modelName]?.status === 'ready';
        set({
          modelDownloads: withEntry(downloads, modelName, status),
          downloadingModel: status.status === 'downloading' ? modelName : null,
        });
        // 首次转为 ready → 模型就绪状态变了，绕过节流刷新探测（可用性标签随之更新）
        if (status.status === 'ready' && !wasReady) {
          await get().probeOllama(true);
        }
      } catch {
        // 轮询异常同样静默：下一次轮询会重试
      }
    },

    importModelPackage: (path) => importModelPackageAction(deps, path),
  };
}
