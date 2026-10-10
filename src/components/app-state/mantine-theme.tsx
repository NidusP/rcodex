import { createTheme, DEFAULT_THEME, MantineProvider, type MantineColorsTuple } from "@mantine/core";
import { Notifications } from "@mantine/notifications";
import type { ReactNode } from "react";
import "./mantine-theme.css";

function channel(value: number) {
  return Math.max(0, Math.min(255, Math.round(value)));
}

function parseHex(hex: string) {
  const match = /^#([0-9a-f]{6})$/i.exec(hex);
  if (!match) return [16, 152, 173] as const;
  const n = Number.parseInt(match[1], 16);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255] as const;
}

function mix(from: readonly number[], to: readonly number[], amount: number) {
  return from.map((part, index) => part + (to[index] - part) * amount);
}

function toHex(rgb: readonly number[]) {
  return "#" + rgb.map((part) => channel(part).toString(16).padStart(2, "0")).join("");
}

/** 把用户选中的一个颜色展开成 Mantine 需要的 10 级色阶。第 6 级是所选颜色。 */
export function accentPalette(hex: string): MantineColorsTuple {
  const base = parseHex(hex);
  const shades = Array.from({ length: 10 }, (_, index) => {
    if (index === 6) return toHex(base);
    if (index < 6) return toHex(mix(base, [255, 255, 255], (6 - index) / 7));
    return toHex(mix(base, [0, 0, 0], ((index - 6) / 4) * 0.72));
  });
  return shades as unknown as MantineColorsTuple;
}

/** 按强调色重建主题。组件尺寸固定为 Mantine 最小档。亮暗只由 color scheme 改写同一组变量。 */
export function createRcodexTheme(accent: string) {
  const sized = { defaultProps: { size: "xs" as const } };
  return createTheme({
    colors: { ...DEFAULT_THEME.colors, accent: accentPalette(accent) },
    primaryColor: "accent",
    primaryShade: { light: 7, dark: 4 },
    autoContrast: true,
    luminanceThreshold: 0.4,
    white: DEFAULT_THEME.white,
    black: DEFAULT_THEME.black,
    fontFamily: "inherit",
    fontFamilyMonospace: "ui-monospace, SFMono-Regular, Menlo, monospace",
    defaultRadius: 8,
    cursorType: "pointer",
    focusRing: "auto",
    respectReducedMotion: true,
    components: {
      Button: sized,
      ActionIcon: sized,
      Checkbox: sized,
      Input: sized,
      TextInput: sized,
      PasswordInput: sized,
      NumberInput: { defaultProps: { size: "xs", allowDecimal: false } },
      Select: {
        defaultProps: {
          size: "xs",
          withCheckIcon: false,
          allowDeselect: false,
          searchable: false,
          comboboxProps: {
            withinPortal: true,
            position: "bottom-start",
            middlewares: { flip: true, shift: true },
            zIndex: 400,
          },
        },
      },
      Menu: sized,
      Tabs: sized,
      Notification: { defaultProps: { withBorder: true, radius: 10 } },
    },
  });
}

/**
 * @component RcodexMantineProvider
 * @description 为控制面提供 Mantine 主题，跟随明暗和强调色。组件尺寸固定为最小档。
 */
export function RcodexMantineProvider({
  colorScheme,
  accent,
  children,
}: {
  colorScheme: "light" | "dark";
  accent: string;
  children: ReactNode;
}) {
  return (
    <MantineProvider
      theme={createRcodexTheme(accent)}
      forceColorScheme={colorScheme}
      defaultColorScheme={colorScheme}
      classNamesPrefix="rcodex"
    >
      <Notifications position="top-right" zIndex={500} containerWidth={430} transitionDuration={180} />
      {children}
    </MantineProvider>
  );
}
