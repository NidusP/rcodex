import { Alert, Group, Stack, Text } from "@mantine/core";
import { AlertTriangle } from "lucide-react";
import { useState } from "react";
import { toCommandError, type LaunchPresetSummary } from "../../codex-control";
import { ControlDialog } from "../control-dialog";
import { HintedButton } from "../hinted-button";

/**
 * @component WorkspaceDeleteDialog
 * @description 明确确认永久删除当前空间的数据，删除失败时保留上下文供重试。
 */
export function WorkspaceDeleteDialog({
  workspace,
  busy,
  blockedReason,
  onClose,
  onConfirm,
}: {
  workspace: LaunchPresetSummary;
  busy: boolean;
  blockedReason?: string;
  onClose: () => void;
  onConfirm: () => Promise<void>;
}) {
  const [error, setError] = useState<string | null>(null);
  return (
    <ControlDialog
      label={`永久删除 ${workspace.name}`}
      onClose={onClose}
      closeDisabled={busy}
      size={480}
      className="control-dialog-confirm"
    >
      <section className="confirm-dialog" role="alertdialog" aria-labelledby="workspace-delete-title" aria-describedby="workspace-delete-copy">
        <AlertTriangle size={24} aria-hidden="true" />
        <Stack gap="sm">
          <h2 id="workspace-delete-title">永久删除 {workspace.name}？</h2>
          <Text id="workspace-delete-copy" size="sm">
            将永久清除这个空间的配置、全部运行记录、隔离会话与文件、Codex 桌面数据和完整请求档案。此操作无法撤销。
          </Text>
          <Text size="xs" c="dimmed">全局模型渠道与密钥、其他隔离空间会保留。</Text>
          {blockedReason ? <Alert color="yellow">{blockedReason}</Alert> : null}
          {error ? <Alert color="red" role="alert" title="删除未完成">{error} 已删除的部分数据可能无法恢复；修复问题后可重试。</Alert> : null}
          <Group justify="flex-end" gap="xs">
            <HintedButton type="button" disabled={busy} onClick={onClose}>取消</HintedButton>
            <HintedButton
              type="button"
              className="danger-action"
              disabled={busy || Boolean(blockedReason)}
              disabledReason={blockedReason ?? (busy ? "正在永久删除空间，请等待完成。" : undefined)}
              onClick={() => {
                setError(null);
                void onConfirm().catch((failure: unknown) => setError(toCommandError(failure).message));
              }}
            >
              {busy ? "正在删除…" : "确认永久删除"}
            </HintedButton>
          </Group>
        </Stack>
      </section>
    </ControlDialog>
  );
}
