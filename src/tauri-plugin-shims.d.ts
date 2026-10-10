declare module "@tauri-apps/plugin-updater" {
  export interface Update {
    version: string;
    date?: string;
    body?: string;
    downloadAndInstall: (onEvent?: (event: unknown) => void) => Promise<void>;
  }
  export function check(options?: Record<string, unknown>): Promise<Update | null>;
}

declare module "@tauri-apps/plugin-process" {
  export function relaunch(): Promise<void>;
}
