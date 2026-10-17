declare module "@tauri-apps/plugin-updater" {
  export type DownloadEvent =
    | { event: "Started"; data: { contentLength?: number } }
    | { event: "Progress"; data: { chunkLength: number } }
    | { event: "Finished" };
  export interface Update {
    version: string;
    date?: string;
    body?: string;
    download: (onEvent?: (event: DownloadEvent) => void) => Promise<void>;
    install: (options?: { restart?: boolean }) => Promise<void>;
    downloadAndInstall: (onEvent?: (event: DownloadEvent) => void) => Promise<void>;
  }
  export function check(options?: Record<string, unknown>): Promise<Update | null>;
}

declare module "@tauri-apps/plugin-process" {
  export function relaunch(): Promise<void>;
}
