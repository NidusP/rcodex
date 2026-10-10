import { Dialog, FocusTrap, Overlay, Portal, Transition } from "@mantine/core";
import { useEffect, type ReactNode } from "react";
import "./control-dialog.css";

const dialogMotionMs = 180;

/**
 * @component ControlDialog
 * @description 控制面对话框。内容和遮罩都淡入淡出，遮罩避开标题栏，忙碌时不能关闭。
 */
export function ControlDialog({
  opened = true,
  onClose,
  closeDisabled = false,
  zIndex = 240,
  size = 460,
  label,
  className,
  children,
}: {
  opened?: boolean;
  onClose: () => void;
  closeDisabled?: boolean;
  zIndex?: number;
  size?: number | string;
  label: string;
  className?: string;
  children: ReactNode;
}) {
  const reduceMotion =
    typeof window !== "undefined" &&
    window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  const motionMs = reduceMotion ? 0 : dialogMotionMs;

  useEffect(() => {
    if (!opened || closeDisabled) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      onClose();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [opened, closeDisabled, onClose]);

  const requestClose = () => {
    if (!closeDisabled) onClose();
  };

  return (
    <>
      <Portal>
        <Transition mounted={opened} transition="fade" duration={motionMs}>
          {(styles) => (
            <Overlay
              fixed
              className="control-dialog-overlay"
              zIndex={Number(zIndex) - 1}
              onClick={requestClose}
              style={{
                ...styles,
                top: "var(--titlebar-h, 40px)",
                height: "calc(100dvh - var(--titlebar-h, 40px))",
              }}
            />
          )}
        </Transition>
      </Portal>
      <Dialog
        opened={opened}
        onClose={requestClose}
        withCloseButton={false}
        withinPortal
        zIndex={zIndex}
        size={size}
        p={0}
        shadow="none"
        withBorder={false}
        radius={0}
        bg="transparent"
        position={{ top: 0, left: 0 }}
        aria-label={label}
        className={["control-dialog-shell", className].filter(Boolean).join(" ")}
        transitionProps={{
          transition: "fade",
          duration: motionMs,
        }}
      >
        <FocusTrap active={opened}>
          <div className="control-dialog-frame">{children}</div>
        </FocusTrap>
      </Dialog>
    </>
  );
}
