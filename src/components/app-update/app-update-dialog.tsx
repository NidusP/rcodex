import { ControlDialog } from "../control-dialog";
import { HintedButton } from "../hinted-button";
import { useAppState } from "../app-state";
import "./app-update-dialog.css";

function formatBytes(bytes: number) {
  if (bytes < 1024) return bytes + " B";
  if (bytes < 1024 * 1024) return (bytes / 1024).toFixed(1) + " KB";
  return (bytes / (1024 * 1024)).toFixed(1) + " MB";
}

/**
 * @component AppUpdateDialog
 * @description 新版本弹窗。先下载当前系统的安装包，下完后才能安装重启。
 */
export function AppUpdateDialog() {
  const {
    appUpdate,
    appUpdateOpen,
    appUpdateBusy,
    appUpdateDownloaded,
    appUpdateProgress,
    closeAppUpdateDialog,
    downloadAppUpdate,
    installAppUpdate,
  } = useAppState();
  if (!appUpdateOpen || !appUpdate) return null;
  const phase = appUpdateProgress?.phase;
  const downloading = appUpdateBusy && phase === "downloading";
  const installing = appUpdateBusy && phase === "installing";
  const downloaded = appUpdateDownloaded || phase === "downloaded";
  const totalBytes = appUpdateProgress?.totalBytes ?? null;
  const receivedBytes = appUpdateProgress?.receivedBytes ?? 0;
  const percent =
    totalBytes && totalBytes > 0
      ? Math.min(100, Math.round((receivedBytes / totalBytes) * 100))
      : null;
  const progressText = !appUpdateProgress
    ? null
    : installing
      ? "正在安装并重启…"
      : downloaded
        ? "本机安装包已就绪"
        : totalBytes
          ? "已下载 " + formatBytes(receivedBytes) + " / " + formatBytes(totalBytes)
          : "已下载 " + formatBytes(receivedBytes);

  return (
    <ControlDialog
      opened={appUpdateOpen}
      onClose={closeAppUpdateDialog}
      closeDisabled={appUpdateBusy}
      size={460}
      label={"更新到 R Codex " + appUpdate.version}
      className="control-dialog-confirm"
    >
      <section className="app-update-dialog" aria-labelledby="app-update-title">
        <div>
          <h2 id="app-update-title">R Codex {appUpdate.version}</h2>
          {appUpdate.body ? (
            <p className="app-update-notes">{appUpdate.body}</p>
          ) : (
            <p>可以下载适合当前系统的安装包。</p>
          )}
          <p>先下载本机对应的安装包，完成后再更新。</p>
        </div>
        {progressText && (
          <div className="app-update-progress" role="status">
            <div
              className={percent === null && !downloaded ? "app-update-bar is-unknown" : "app-update-bar"}
              aria-hidden="true"
            >
              <span style={percent === null || downloaded ? { width: "100%" } : { width: percent + "%" }} />
            </div>
            <p>{progressText}</p>
          </div>
        )}
        <div className="app-update-actions">
          <HintedButton
            className="secondary-action"
            type="button"
            disabled={appUpdateBusy}
            onClick={closeAppUpdateDialog}
          >
            取消
          </HintedButton>
          <HintedButton
            className="secondary-action"
            type="button"
            disabled={appUpdateBusy || downloaded}
            title={downloaded ? "安装包已下载" : "下载适合当前系统的安装包"}
            onClick={() => void downloadAppUpdate()}
          >
            {downloading ? "正在下载…" : downloaded ? "已下载" : "下载"}
          </HintedButton>
          <HintedButton
            className="primary-action"
            type="button"
            disabled={!downloaded || appUpdateBusy}
            title={downloaded ? "安装并重启" : "请先下载安装包"}
            onClick={() => void installAppUpdate()}
          >
            {installing ? "正在更新…" : "更新"}
          </HintedButton>
        </div>
      </section>
    </ControlDialog>
  );
}
