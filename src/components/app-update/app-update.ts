import { APP_RELEASE_PAGE } from "../../app-release";

export type AppReleaseUpdate = {
  version: string;
  body?: string | null;
  date?: string | null;
  install: () => Promise<void>;
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
      install: async () => {
        await update.downloadAndInstall();
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
