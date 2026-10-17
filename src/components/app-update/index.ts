/**
 * R Codex 自身的 GitHub 发版检查与安装。
 * 不检查 Codex CLI。对外导出检查、下载弹窗和打开发布页。
 */
export {
  FORCE_APP_UPDATE_PREVIEW,
  previewAppUpdate,
  checkAppRelease,
  openAppReleasePage,
  type AppReleaseUpdate,
  type AppUpdateProgress,
} from "./app-update";
export { AppUpdateDialog } from "./app-update-dialog";
