import { Download, RotateCw, SunMoon } from "lucide-react";
import { ColorPicker, ColorSwatch, Menu, Popover } from "@mantine/core";
import { HintedButton } from "../hinted-button";
import { useAppState } from "../app-state";
import { type ThemeMode } from "../../app-model";
import "./title-bar.css";

const themeLabels: Record<ThemeMode, string> = {
  system: "跟随系统",
  light: "亮色",
  dark: "暗色",
};

/**
 * @component ThemeControls
 * @description 页面里的明暗和强调色。没有单独的主题页。
 */
export function ThemeControls() {
  const { themeMode, setThemeMode, accent, setAccent } = useAppState();
  return (
    <div className="titlebar-theme">
      <Menu position="bottom-end" withinPortal>
        <Menu.Target>
          <HintedButton className="icon-button" type="button" title={"明暗：" + themeLabels[themeMode]} aria-label="明暗">
            <SunMoon size={16} aria-hidden="true" />
          </HintedButton>
        </Menu.Target>
        <Menu.Dropdown>
          {(Object.keys(themeLabels) as ThemeMode[]).map((mode) => (
            <Menu.Item key={mode} onClick={() => setThemeMode(mode)}>
              {themeLabels[mode]}
            </Menu.Item>
          ))}
        </Menu.Dropdown>
      </Menu>
      <Popover position="bottom-end" withinPortal shadow="md" radius="md">
        <Popover.Target>
          <ColorSwatch className="titlebar-swatch" component="button" type="button" color={accent} aria-label="强调色" title="强调色" />
        </Popover.Target>
        <Popover.Dropdown>
          <ColorPicker className="titlebar-color-picker" format="hex" value={accent} onChange={setAccent} />
        </Popover.Dropdown>
      </Popover>
    </div>
  );
}

/** 判断当前窗口装饰风格：mac、windows 或 web。 */
export function desktopChrome(): "mac" | "windows" | "web" {
  if (typeof window === "undefined") return "web";
  const tauri = "__TAURI_INTERNALS__" in window || "__TAURI__" in window;
  if (!tauri) return "web";
  if (/Windows/i.test(navigator.userAgent)) return "windows";
  if (/Mac/i.test(navigator.userAgent)) return "mac";
  return "web";
}

/**
 * @component AppUpdateButton
 * @description 有新版本时显示的安装入口。
 */
export function AppUpdateButton() {
  const { appUpdate, appUpdateBusy, openAppUpdateDialog } = useAppState();
  if (!appUpdate) return null;
  return (
    <HintedButton
      className="icon-button"
      type="button"
      title={appUpdateBusy ? "正在下载更新" : "下载更新"}
      aria-label="下载更新"
      disabled={appUpdateBusy}
      onClick={openAppUpdateDialog}
    >
      <Download size={16} aria-hidden="true" />
    </HintedButton>
  );
}

/**
 * @component GatewayRestartButton
 * @description 只在 Gateway 失联时出现，放在页面里的 R Codex 旁边。
 */
export function GatewayRestartButton() {
  const { gateway, gatewayRestartBusy, restartLostGateway } = useAppState();
  if (gateway.state !== "unavailable") return null;
  return (
        <HintedButton
          className="gateway-restart"
          type="button"
      title={gatewayRestartBusy ? "正在重启失联的 Gateway" : "关闭失联的旧 Gateway，并启动新的 sidecar。正在进行的请求会中断，活动绑定会从数据库恢复。"}
      aria-label={gatewayRestartBusy ? "正在重启 Gateway" : "重启 Gateway"}
      disabled={gatewayRestartBusy}
      onClick={() => void restartLostGateway()}
        >
          <RotateCw size={14} aria-hidden="true" />
          {gatewayRestartBusy ? "重启中" : "重启"}
        </HintedButton>
  );
}
