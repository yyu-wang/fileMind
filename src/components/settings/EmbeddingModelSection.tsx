// Embedding 模型区块（设置页）：展示当前模型状态并提供模型下载。
//
// 背景：Embedding 由 Sidecar 进程内 ONNX 推理完成，模型文件需先下载到本机
// （设置页触发 → Sidecar 从 HF 镜像下载）。**未下载完成前，索引构建与知识问答
// 不可用**，故此处给出明确的进度与文案。
// 切换模型需重建全部索引，P1 未支持，不渲染切换占位按钮。
// 原型 05_交互原型 §设置页 Embedding 管理：.panel 展示当前模型 + .model-item 列表。
//
// 下载状态按 model_name 存放在 store 的 modelDownloads 映射里（不再用单槽），
// 同一页上的 Embedding / Rerank / 本地 GGUF 三张卡片因此互不覆盖。

import { useEffect } from 'react';
import type { EmbeddingModelAvailability, ModelDownloadStatus } from '@/types/ipc';
import { formatMb, percentOf } from '@/lib/modelDownloadFormat';
import { useFileStore } from '../../stores/fileStore';
import { useSettingsStore } from '../../stores/settingsStore';

/** 下载进度轮询间隔（ms）：Sidecar 侧为后台任务，靠轮询推进度。 */
export const DOWNLOAD_POLL_MS = 1_000;

/** 下载中：进度条 + 文案（百分比 / 已下载量 / 镜像 / 尝试次数）。 */
function DownloadProgress({ download }: { download: ModelDownloadStatus }) {
  const percent = percentOf(download);
  const amount =
    percent === null
      ? `已下载 ${formatMb(download.downloaded_bytes)}`
      : `${percent}%（${formatMb(download.downloaded_bytes)} / ${formatMb(download.total_bytes ?? 0)}）`;
  return (
    <div style={{ marginTop: 12 }} data-testid="model-download-progress">
      <div
        className="model-progress"
        role="progressbar"
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={percent ?? undefined}
      >
        <div className="model-progress__bar" style={{ width: `${percent ?? 0}%` }} />
      </div>
      <div style={{ fontSize: 12, color: 'var(--muted)', marginTop: 'var(--sp-2)' }}>
        正在下载模型文件：{amount}
        {download.attempt > 1 ? ` · 第 ${download.attempt} 次尝试` : ''}
        {download.mirror ? ` · 镜像 ${download.mirror}` : ''}
      </div>
    </div>
  );
}

/** 自动重试用尽：展示原因并提供重新下载入口（由列表按钮触发）。 */
function DownloadFailure({ download }: { download: ModelDownloadStatus }) {
  return (
    <div style={{ marginTop: 12 }} role="alert" data-testid="model-download-failure">
      <div style={{ fontSize: 12, color: 'var(--danger, #d33)' }}>
        下载失败：{download.error ?? '未知原因'}（已自动重试并切换镜像，请点击重试）
      </div>
    </div>
  );
}

interface ModelRowProps {
  model: EmbeddingModelAvailability;
  isCurrent: boolean;
  /** 该模型自己的下载状态（按 model_name 从映射里取；未查询过为 null） */
  download: ModelDownloadStatus | null;
  onDownload: (name: string) => void;
}

/** 单个模型行：就绪/未下载徽标 + 下载按钮。 */
function ModelRow({ model, isCurrent, download, onDownload }: ModelRowProps) {
  const isDownloading = download?.status === 'downloading';
  const hasFailed = download?.status === 'failed';
  return (
    <div className={`model-item${isCurrent ? ' model-item--current' : ''}`}>
      <div className="model-info">
        <div className="name">{model.name}</div>
        <div className="meta">
          dim {model.dim} · v{model.version}
          {model.available ? '' : ' · 未下载'}
        </div>
      </div>
      <span className={`tag ${model.available ? 'tag-green' : 'tag-gray'}`}>
        {model.available ? '已就绪' : '未下载'}
      </span>
      {!model.available && (
        <button
          type="button"
          className="btn btn--primary btn--sm"
          data-testid="model-download-btn"
          disabled={isDownloading}
          onClick={() => onDownload(model.name)}
          title={isDownloading ? '正在下载中' : '下载该模型文件'}
        >
          {isDownloading ? '下载中...' : hasFailed ? '重新下载' : '下载'}
        </button>
      )}
    </div>
  );
}

export function EmbeddingModelSection() {
  const embeddingModel = useSettingsStore((s) => s.embeddingModel);
  const embeddingModelOptions = useSettingsStore((s) => s.embeddingModelOptions);
  const downloads = useSettingsStore((s) => s.modelDownloads);
  const installError = useSettingsStore((s) => s.installErrors[embeddingModel] ?? null); // 只读本模型那条
  const startModelDownload = useSettingsStore((s) => s.startModelDownload);
  const refreshModelDownload = useSettingsStore((s) => s.refreshModelDownload);
  const stats = useFileStore((s) => s.stats);
  const loadStats = useFileStore((s) => s.loadStats);

  const current = embeddingModelOptions.find((m) => m.name === embeddingModel);
  const download = downloads[embeddingModel] ?? null;
  const isDownloading = download?.status === 'downloading';

  useEffect(() => {
    if (!stats) void loadStats();
  }, [stats, loadStats]);

  // 进页查一次状态：从设置页切走再回来时能接着显示进行中的进度
  useEffect(() => {
    void refreshModelDownload(embeddingModel);
  }, [embeddingModel, refreshModelDownload]);

  // 下载中按固定间隔轮询；结束（ready/failed）后自动停止
  useEffect(() => {
    if (!isDownloading) return;
    const timer = window.setInterval(() => {
      void refreshModelDownload(embeddingModel);
    }, DOWNLOAD_POLL_MS);
    return () => window.clearInterval(timer);
  }, [isDownloading, embeddingModel, refreshModelDownload]);

  return (
    <section className="settings-section" aria-labelledby="settings-embedding-title">
      <h3 id="settings-embedding-title" className="settings-section__title">
        📐 Embedding 模型管理
      </h3>
      <p className="section-desc">
        模型文件需下载到本机后才能使用；下载完成前，建立索引与知识问答不可用。
      </p>

      <div className="panel" style={{ marginBottom: 12 }}>
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
          <div>
            <div
              style={{ fontSize: 14, fontWeight: 600, color: 'var(--ink)' }}
              data-testid="embedding-current"
            >
              当前模型: {embeddingModel}
              {current ? ` · dim ${current.dim} · v${current.version}` : ''}
            </div>
            <div style={{ fontSize: 12, color: 'var(--muted)', marginTop: 2 }}>
              已索引 {(stats?.categorized_files ?? 0).toLocaleString()} 文件 · 向量块数未知
            </div>
          </div>
          <span className="tag tag-green" title="当前模型已锁定，切换需重建索引">
            已锁定
          </span>
        </div>

        {current?.available === true && !isDownloading && (
          <div
            style={{ marginTop: 12, fontSize: 12, color: 'var(--muted)' }}
            data-testid="model-ready"
          >
            模型文件已就绪，可用于建立索引与知识问答。
          </div>
        )}
        {isDownloading && download && <DownloadProgress download={download} />}
        {download?.status === 'failed' && <DownloadFailure download={download} />}
      </div>

      {installError && (
        <p className="section-desc" role="alert" style={{ color: 'var(--danger, #d33)' }}>
          下载请求失败: {installError}
        </p>
      )}

      <div className="section-desc" style={{ marginBottom: 8 }}>
        可用 Embedding 模型：
      </div>
      {embeddingModelOptions.length === 0 ? (
        <div className="model-item">
          <div className="model-info">
            <div className="name">暂无模型列表</div>
            <div className="meta">请先在设置页完成本地环境检测</div>
          </div>
        </div>
      ) : (
        embeddingModelOptions.map((model) => (
          <ModelRow
            key={model.name}
            model={model}
            isCurrent={model.name === embeddingModel}
            download={downloads[model.name] ?? null}
            onDownload={(name) => void startModelDownload(name)}
          />
        ))
      )}
    </section>
  );
}
