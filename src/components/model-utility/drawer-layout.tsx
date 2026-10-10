import type { ReactNode } from "react";
import { Drawer } from "@mantine/core";
import { ArrowLeft } from "lucide-react";
import { HintedButton } from "../hinted-button";
import "./drawer-layout.css";

/** 控制面抽屉避开窗口标题栏，避免盖住系统按钮。 */
export const overlayDrawerOffset = {
  top: "var(--titlebar-h, 40px)",
  height: "calc(100dvh - var(--titlebar-h, 40px))",
} as const;

/** Mantine Drawer 的控制面样式钩子，由 CSS 覆盖外观。 */
export const controlDrawerClassNames = {
  root: "control-drawer",
  inner: "control-drawer-inner",
  content: "control-drawer-content",
  header: "control-drawer-header",
  title: "control-drawer-title",
  body: "control-drawer-body",
  overlay: "control-drawer-overlay",
} as const;

const drawerMotionMs = 180;

/**
 * @component ControlDrawer
 * @description 控制面共用的 Mantine 抽屉。遮罩和抽屉都淡入淡出，避开标题栏，忙碌时禁止关闭。
 */
export function ControlDrawer({
  opened,
  onClose,
  title,
  size = "100%",
  zIndex = 40,
  closeDisabled = false,
  withBackButton = false,
  children,
}: {
  opened: boolean;
  onClose: () => void;
  title: string;
  size?: string | number;
  /** 叠在其他控制面抽屉之上时提高。模型渠道是 40，添加渠道用更高的一层。 */
  zIndex?: number;
  closeDisabled?: boolean;
  withBackButton?: boolean;
  children: ReactNode;
}) {
  const reduceMotion =
    typeof window !== "undefined" &&
    window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  const motionMs = reduceMotion ? 0 : drawerMotionMs;
  const heading = withBackButton ? (
    <div className="control-drawer-heading">
      <HintedButton
        className="icon-button control-drawer-back"
        type="button"
        size="md"
        aria-label="返回"
        disabled={closeDisabled}
        onClick={onClose}
      >
        <ArrowLeft size={18} aria-hidden="true" />
      </HintedButton>
      <span>{title}</span>
    </div>
  ) : (
    title
  );
  return (
    <Drawer
      opened={opened}
      onClose={() => {
        if (!closeDisabled) onClose();
      }}
      title={heading}
      position="right"
      size={size}
      padding="lg"
      zIndex={zIndex}
      shadow="xl"
      withOverlay
      withCloseButton={!withBackButton}
      closeOnClickOutside={!closeDisabled}
      closeOnEscape={!closeDisabled}
      lockScroll
      trapFocus
      transitionProps={{ transition: "fade", duration: motionMs }}
      overlayProps={{
        backgroundOpacity: 0.28,
        blur: 4,
        transitionProps: { transition: "fade", duration: motionMs },
      }}
      classNames={controlDrawerClassNames}
      styles={{ inner: overlayDrawerOffset, overlay: overlayDrawerOffset }}
    >
      {children}
    </Drawer>
  );
}
