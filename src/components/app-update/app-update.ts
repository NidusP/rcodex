import { APP_RELEASE_PAGE } from "../../app-release";
import type { DownloadEvent } from "@tauri-apps/plugin-updater";

export type AppReleaseUpdate = {
  version: string;
  body?: string | null;
  date?: string | null;
  download: (onProgress?: (progress: AppUpdateProgress) => void) => Promise<void>;
  install: () => Promise<void>;
};

export type AppUpdateProgress = {
  receivedBytes: number;
  totalBytes: number | null;
  phase: "downloading" | "downloaded" | "installing";
};

/** 预览下载弹窗：为 true 时标题栏始终显示下载按钮，不检查 GitHub。看完样式后改回 false。 */
export const FORCE_APP_UPDATE_PREVIEW = false;

function applyDownloadEvent(
  event: DownloadEvent,
  state: { receivedBytes: number; totalBytes: number | null },
  onProgress?: (progress: AppUpdateProgress) => void,
) {
  if (event.event === "Started") {
    state.totalBytes = event.data.contentLength ?? null;
    state.receivedBytes = 0;
    onProgress?.({ ...state, phase: "downloading" });
    return;
  }
  if (event.event === "Progress") {
    state.receivedBytes += event.data.chunkLength;
    onProgress?.({ ...state, phase: "downloading" });
    return;
  }
  onProgress?.({
    receivedBytes: state.totalBytes ?? state.receivedBytes,
    totalBytes: state.totalBytes,
    phase: "downloaded",
  });
}

async function wait(ms: number) {
  await new Promise((resolve) => window.setTimeout(resolve, ms));
}

export function previewAppUpdate(): AppReleaseUpdate {
  return {
    version: "0.1.5",
    body: "这是下载弹窗样式预览，不会真正下载或安装。",
    date: null,
    download: async (onProgress) => {
      const totalBytes = 8 * 1024 * 1024;
      onProgress?.({ receivedBytes: 0, totalBytes, phase: "downloading" });
      for (let step = 1; step <= 8; step += 1) {
        await wait(120);
        onProgress?.({
          receivedBytes: Math.round((totalBytes * step) / 8),
          totalBytes,
          phase: "downloading",
        });
      }
      onProgress?.({ receivedBytes: totalBytes, totalBytes, phase: "downloaded" });
    },
    install: async () => {
      await wait(400);
    },
  };
}

export async function checkAppRelease(): Promise<AppReleaseUpdate | null> {
  if (FORCE_APP_UPDATE_PREVIEW) return previewAppUpdate();
  try {
    const { check } = await import("@tauri-apps/plugin-updater");
    const update = await check();
    if (!update) return null;
    return {
      version: update.version,
      body: update.body,
      date: update.date,
      download: async (onProgress) => {
        const state = { receivedBytes: 0, totalBytes: null as number | null };
        await update.download((event: DownloadEvent) => {
          applyDownloadEvent(event, state, onProgress);
        });
        onProgress?.({
          receivedBytes: state.totalBytes ?? state.receivedBytes,
          totalBytes: state.totalBytes,
          phase: "downloaded",
        });
      },
      install: async () => {
        await update.install();
        try {
          const process = await import("@tauri-apps/plugin-process");
          await process.relaunch();
        } catch {
          // Windows install() already exits after launching the installer.
        }
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
