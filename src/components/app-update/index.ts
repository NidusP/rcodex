/**
 * R Codex 自身的 GitHub 发版检查与安装。
 * 不检查 Codex CLI。对外只导出检查、安装和打开发布页。
 */
export {
  checkAppRelease,
  openAppReleasePage,
  type AppReleaseUpdate,
} from "./app-update";
