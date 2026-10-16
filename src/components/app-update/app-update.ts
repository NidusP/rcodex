import { APP_RELEASE_PAGE } from "../../app-release";
import type { DownloadEvent } from "@tauri-apps/plugin-updater";

export type AppReleaseUpdate = {
  version: string;
  body?: string | null;
  date?: string | null;
  install: (onProgress?: (progress: AppUpdateProgress) => void) => Promise<void>;
};

export type AppUpdateProgress = {
  receivedBytes: number;
  totalBytes: number | null;
  phase: "downloading" | "installing";
};

export async function checkAppRelease(): Promise<AppReleaseUpdate | null> {
  try {
    const { check } = await import("@tauri-apps/plugin-updater");
    const update = await check();
    if (!update) return null;
    return {
      version: update.version,
      body: update.body,
      date: update.date,
      install: async (onProgress) => {
        let receivedBytes = 0;
        let totalBytes: number | null = null;
        await update.downloadAndInstall((event: DownloadEvent) => {
          if (event.event === "Started") {
            totalBytes = event.data.contentLength ?? null;
            receivedBytes = 0;
            onProgress?.({ receivedBytes, totalBytes, phase: "downloading" });
            return;
          }
          if (event.event === "Progress") {
            receivedBytes += event.data.chunkLength;
            onProgress?.({ receivedBytes, totalBytes, phase: "downloading" });
            return;
          }
          onProgress?.({
            receivedBytes: totalBytes ?? receivedBytes,
            totalBytes,
            phase: "installing",
          });
        });
        const process = await import("@tauri-apps/plugin-process");
        await process.relaunch();
      },
    };
  } catch {
    return null;
  }
}

export async function openAppReleasePage(): Promise<void> {
  try {
    const { openUrl } = await import("@tauri-apps/plugin-opener");
    await openUrl(APP_RELEASE_PAGE);
  } catch {
    window.open(APP_RELEASE_PAGE, "_blank", "noopener");
  }
}
