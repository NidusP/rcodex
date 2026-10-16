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
 * @description 新版本下载弹窗。标题栏按钮只负责打开，确认后才下载并显示进度。
 */
export function AppUpdateDialog() {
  const {
    appUpdate,
    appUpdateOpen,
    appUpdateBusy,
    appUpdateProgress,
    closeAppUpdateDialog,
    installAppUpdate,
  } = useAppState();
  if (!appUpdateOpen || !appUpdate) return null;
  const totalBytes = appUpdateProgress?.totalBytes ?? null;
  const receivedBytes = appUpdateProgress?.receivedBytes ?? 0;
  const percent =
    totalBytes && totalBytes > 0
      ? Math.min(100, Math.round((receivedBytes / totalBytes) * 100))
      : null;
  const progressText = !appUpdateProgress
    ? null
    : appUpdateProgress.phase === "installing"
      ? "下载完成，正在安装并重启…"
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
            <p>可以下载并安装这个版本。</p>
          )}
        </div>
        {progressText && (
          <div className="app-update-progress" role="status">
            <div
              className={percent === null ? "app-update-bar is-unknown" : "app-update-bar"}
              aria-hidden="true"
            >
              <span style={percent === null ? undefined : { width: percent + "%" }} />
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
            className="primary-action"
            type="button"
            disabled={appUpdateBusy}
            onClick={() => void installAppUpdate()}
          >
            {appUpdateBusy ? "正在更新…" : "下载并更新"}
          </HintedButton>
        </div>
      </section>
    </ControlDialog>
  );
}
