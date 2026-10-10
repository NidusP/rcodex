import { ActionIcon, Button, Tooltip } from "@mantine/core";
import type { MantineSize } from "@mantine/core";
import type { ButtonHTMLAttributes, ReactNode } from "react";
import "./hinted-button.css";

type NativeButtonProps = Omit<
  ButtonHTMLAttributes<HTMLButtonElement>,
  "color" | "style"
>;

function looksFromClass(className?: string) {
  const value = className ?? "";
  if (value.includes("icon-plain")) {
    return { kind: "icon" as const, variant: "transparent" as const, color: undefined };
  }
  if (
    value.includes("icon-button") ||
    value.includes("circular-button") ||
    value.includes("window-close") ||
    value.includes("window-caption-btn") ||
    value.includes("channel-drag")
  ) {
    return { kind: "icon" as const, variant: "subtle" as const, color: undefined };
  }
  if (value.includes("danger")) {
    return { kind: "button" as const, variant: "filled" as const, color: "red" };
  }
  if (value.includes("text-button") || value.includes("notice-link")) {
    return { kind: "button" as const, variant: "subtle" as const, color: undefined };
  }
  if (value.includes("control-app") || value.includes("primary-action")) {
    return { kind: "button" as const, variant: "filled" as const, color: undefined };
  }
  return { kind: "button" as const, variant: "default" as const, color: undefined };
}

/**
 * @component HintedButton
 * @description Mantine 按钮封装。提示使用 Tooltip，禁用时说明原因。
 */
export function HintedButton({
  disabledReason,
  title,
  disabled = false,
  children,
  className,
  type = "button",
  size,
  ...props
}: NativeButtonProps & {
  disabledReason?: string;
  children?: ReactNode;
  size?: MantineSize;
}) {
  const hint = (disabled ? (disabledReason ?? title) : title)?.trim();
  const looks = looksFromClass(className);
  const control =
    looks.kind === "icon" ? (
      <ActionIcon
        className={className}
        type={type}
        variant={looks.variant}
        disabled={disabled}
        size={size}
        {...props}
      >
        {children}
      </ActionIcon>
    ) : (
      <Button
        className={className}
        type={type}
        variant={looks.variant}
        color={looks.color}
        disabled={disabled}
        size={size}
        {...props}
      >
        {children}
      </Button>
    );
  if (!hint) return control;
  const target = disabled ? (
    <span
      className="disabled-hint"
      tabIndex={0}
      aria-label={"当前不可用：" + hint}
    >
      {control}
    </span>
  ) : (
    control
  );
  return (
    <Tooltip
      label={hint}
      multiline
      maw={280}
      withArrow
      openDelay={300}
      zIndex={450}
      position="top"
      events={{ hover: true, focus: true, touch: true }}
      classNames={{ tooltip: "control-tooltip" }}
    >
      {target}
    </Tooltip>
  );
}
