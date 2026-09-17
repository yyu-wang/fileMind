// 离线模型包导入区块（设置页）：内网 / 无外网部署时用另一台已下载好模型的机器分发模型文件。
//
// 背景：Embedding / Rerank 模型默认由 Sidecar 从 HF 镜像下载，内网机器拿不到外网，
// 检索时会因「没有对应的模型」不可用。此处的导入把另一台机器上的 `models` 目录
// （或其 zip 包）拷贝到本机，复用同一套模型文件布局，无需重新下载。
//
// 与下载区块的差异：导入是一次性本地拷贝（数十秒、后端不提供进度），故不轮询状态，
// 只按 store 的 importingPackage / importResult 反映进行中与结果。

import { open } from '@tauri-apps/plugin-dialog';
import type { ModelImportResult } from '@/types/ipc';
import { useSettingsStore } from '../../stores/settingsStore';

/** 对话框返回值收敛为单个路径：用户取消（null）与多选（数组）都不是有效包路径。 */
function singlePath(selected: string | string[] | null): string | null {
  return typeof selected === 'string' ? selected : null;
}

/** 结果文案：分别列出本次导入与幂等跳过的模型；两者皆空说明包内模型本机都已就绪。 */
function resultText(result: ModelImportResult): string {
  const parts: string[] = [];
  if (result.imported.length > 0) parts.push(`已导入：${result.imported.join('、')}`);
  if (result.skipped.length > 0) parts.push(`已就绪跳过：${result.skipped.join('、')}`);
  if (parts.length === 0) return '包内模型均已在本机就绪';
  return parts.join('；');
}

interface ImportActionsProps {
  isImporting: boolean;
  onPickZip: () => void;
  onPickDirectory: () => void;
}

/** 两个导入入口：zip 包 / 已解压的模型目录（导入中统一禁用，防重复提交）。 */
function ImportActions({ isImporting, onPickZip, onPickDirectory }: ImportActionsProps) {
  const label = (text: string) => (isImporting ? '导入中…' : text);
  const title = (text: string) => (isImporting ? '正在导入中' : text);
  return (
    <div className="settings-row settings-row--actions">
      <button
        type="button"
        className="btn btn--primary btn--sm"
        data-testid="offline-import-zip-btn"
        disabled={isImporting}
        onClick={onPickZip}
        title={title('选择离线模型 zip 包')}
      >
        {label('导入 zip 包')}
      </button>
      <button
        type="button"
        className="btn btn--ghost btn--sm"
        data-testid="offline-import-dir-btn"
        disabled={isImporting}
        onClick={onPickDirectory}
        title={title('选择已下载好模型的 models 目录')}
      >
        {label('导入模型目录')}
      </button>
    </div>
  );
}

/** 区块说明：包内容要求 + 导入耗时 + 网络共享盘提示。 */
function PackageHint() {
  return (
    <p className="section-desc">
      本机无外网时，可将另一台已下载好模型的机器上的 models 目录（或其 zip 包）导入； 包内需含
      bge-large-zh-v1.5/ 或 bge-reranker-v2-m3/ 等模型目录。导入为本地拷贝，
      需要数十秒，过程中请勿关闭应用。若包放在网络共享盘，请先拷到本机再导入： 导入请求有 10
      分钟上限，数 GB 的包走慢速共享盘可能中途超时。
    </p>
  );
}

export function OfflineModelSection() {
  const importingPackage = useSettingsStore((s) => s.importingPackage);
  const importResult = useSettingsStore((s) => s.importResult);
  // 专用错误字段（非通用 error）：通用 error 由 Ollama 探测写入且每次探测重置，
  // 直接复用会在「未导入」时误显示探测错误、或在导入失败后被探测擦掉
  const importError = useSettingsStore((s) => s.importError);
  const importModelPackage = useSettingsStore((s) => s.importModelPackage);

  /** 选 zip 包（只允许 .zip，避免误把目录或无关文件当包） */
  const handlePickZip = async () => {
    const selected = singlePath(
      await open({
        multiple: false,
        directory: false,
        filters: [{ name: '离线模型包', extensions: ['zip'] }],
      }),
    );
    // 用户取消：不触发后端（导入耗时长，误触发代价高）
    if (selected === null) return;
    await importModelPackage(selected);
  };

  /** 选已解压的模型目录（`models` 目录或单个模型目录） */
  const handlePickDirectory = async () => {
    const selected = singlePath(await open({ multiple: false, directory: true }));
    if (selected === null) return;
    await importModelPackage(selected);
  };

  return (
    <section className="settings-section" aria-labelledby="settings-offline-model-title">
      <h3 id="settings-offline-model-title" className="settings-section__title">
        📦 离线模型包（内网部署）
      </h3>
      <PackageHint />

      <ImportActions
        isImporting={importingPackage}
        onPickZip={() => void handlePickZip()}
        onPickDirectory={() => void handlePickDirectory()}
      />

      {importResult && (
        <p className="section-desc" data-testid="offline-import-result">
          {resultText(importResult)}
        </p>
      )}

      {importError && (
        <p className="settings-section__error" role="alert" data-testid="offline-import-error">
          导入失败：{importError}
        </p>
      )}
    </section>
  );
}
