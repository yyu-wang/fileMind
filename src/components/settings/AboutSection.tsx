// 关于区块（设置页）：版本 / 技术栈 / 索引数据库 + 检查更新 / 导出配置 / 重置应用。
//
// 原型 05_交互原型 §设置页关于区。版本号与 package.json 保持一致（手动同步）。
// 「检查更新」接入 tauri-plugin-updater：手动触发检查 → 发现新版则二次确认 →
// 下载并安装 → process 插件重启应用。「导出配置 / 重置应用」P1 不支持，留 disabled。

import { useState } from 'react';
import { check } from '@tauri-apps/plugin-updater';
import { relaunch } from '@tauri-apps/plugin-process';

import { ConfirmDialog } from '../ui/ConfirmDialog';

/** 应用版本：与 package.json 的 version 字段保持一致。 */
const APP_VERSION = 'v0.1.0';

/** 检查接口返回的更新对象类型（避免手写 any）。 */
type UpdateInfo = NonNullable<Awaited<ReturnType<typeof check>>>;

export function AboutSection() {
  const [checking, setChecking] = useState(false);
  const [installing, setInstalling] = useState(false);
  const [available, setAvailable] = useState<UpdateInfo | null>(null);
  const [status, setStatus] = useState<string | null>(null);

  /** 手动检查更新：有新版本弹确认框；无版本提示「已是最新」。 */
  const handleCheckUpdate = async () => {
    if (checking) return;
    setChecking(true);
    setStatus(null);
    try {
      const update = await check();
      if (update) {
        setAvailable(update);
      } else {
        setStatus('已是最新版本');
      }
    } catch (err) {
      setStatus(`检查更新失败：${err instanceof Error ? err.message : String(err)}`);
    } finally {
      setChecking(false);
    }
  };

  /** 确认安装：下载并安装完成后调用 relaunch 重启应用。 */
  const handleInstall = async () => {
    if (!available) return;
    setInstalling(true);
    try {
      await available.downloadAndInstall();
      setStatus('更新已安装，正在重启应用…');
      setAvailable(null);
      await relaunch();
    } catch (err) {
      setStatus(`安装更新失败：${err instanceof Error ? err.message : String(err)}`);
      setAvailable(null);
    } finally {
      setInstalling(false);
    }
  };

  return (
    <section className="settings-section" aria-labelledby="settings-about-title">
      <h3 id="settings-about-title" className="settings-section__title">
        ℹ️ 关于
      </h3>
      <div className="setting-row">
        <div className="setting-label">
          <div className="name">版本</div>
        </div>
        <div className="setting-control">
          <span className="mono text-muted" data-testid="about-version">
            {APP_VERSION}
          </span>
        </div>
      </div>
      <div className="setting-row">
        <div className="setting-label">
          <div className="name">技术栈</div>
        </div>
        <div className="setting-control">
          <span className="text-muted text-sm">
            Tauri 2 · React 19 · Python FastAPI · LangChain
          </span>
        </div>
      </div>
      <div className="setting-row">
        <div className="setting-label">
          <div className="name">索引数据库</div>
        </div>
        <div className="setting-control">
          <span className="text-muted text-sm">SQLite + LanceDB</span>
        </div>
      </div>
      <div className="setting-row" style={{ borderBottom: 'none', padding: '12px 0 0' }}>
        <div className="setting-control" style={{ display: 'flex', gap: 10 }}>
          <button
            type="button"
            className="btn btn--ghost btn--sm"
            data-testid="about-check-update"
            disabled={checking || installing}
            onClick={() => void handleCheckUpdate()}
          >
            {checking ? '检查中…' : '检查更新'}
          </button>
          <button
            type="button"
            className="btn btn--ghost btn--sm"
            disabled
            title="P1 不支持，敬请期待"
          >
            导出配置
          </button>
          <button
            type="button"
            className="btn btn--ghost btn--sm"
            disabled
            title="P1 不支持，敬请期待"
            style={{ color: 'var(--warn)' }}
          >
            重置应用
          </button>
        </div>
      </div>
      {status && (
        <p
          className={`settings-section__desc settings-section__about-status${
            status.startsWith('已是最新') ? '' : ' settings-section__error'
          }`}
          role="status"
          data-testid="about-update-status"
        >
          {status}
        </p>
      )}
      {available && (
        <ConfirmDialog
          title="发现新版本"
          message={`FileMind ${available.version} 已可用${available.body ? `：\n${available.body}` : ''}。是否现在下载并安装？`}
          confirmLabel={installing ? '安装中…' : '下载并安装'}
          danger={false}
          loading={installing}
          onConfirm={() => void handleInstall()}
          onCancel={() => {
            if (!installing) setAvailable(null);
          }}
        />
      )}
    </section>
  );
}
