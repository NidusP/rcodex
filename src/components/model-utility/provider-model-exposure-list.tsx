import { Alert, Checkbox, Group, Stack, Text, Tooltip } from "@mantine/core";
import { RefreshCw } from "lucide-react";
import type { DiscoveredModel, Provider } from "../../app-model";
import { formatTime } from "../../app-model";
import { imageProbeErrorHint, imageProbeLabel } from "../../model-exposure";
import { useAppState } from "../app-state";
import { HintedButton } from "../hinted-button";
import { catalogModelKind, catalogModelPurpose } from "../model-labels";

/**
 * @component ProviderModelExposureList
 * @description 渠道模型目录中的显式透出、图片检测状态与行级刷新操作。
 */
export function ProviderModelExposureList({
  provider,
  models,
  wireApi,
}: {
  provider: Provider;
  models: DiscoveredModel[];
  wireApi: "responses" | "chat_completions" | null;
}) {
  const {
    exposuresByProvider,
    exposureErrorsByProvider,
    modelExposurePending,
    modelExposureError,
    changeModelExposure,
    refreshModelImageProbe,
    refreshProviderExposures,
  } = useAppState();
  const exposures = exposuresByProvider[provider.id];
  const error = exposureErrorsByProvider[provider.id];
  const discoveredIds = new Set(models.map((model) => model.upstreamModelId));
  const rows = [
    ...models,
    ...(exposures ?? [])
      .filter((model) => model.exposed && !discoveredIds.has(model.upstreamModelId))
      .map((model) => ({ upstreamModelId: model.upstreamModelId, displayName: model.upstreamModelId, ownedBy: null })),
  ];

  return (
    <Stack gap="xs">
      {error ? (
        <Alert color="red" title="模型透出状态不可用" role="alert">
          <Text size="xs">{error}</Text>
          <HintedButton type="button" onClick={() => void refreshProviderExposures(provider.id)}>重试读取</HintedButton>
        </Alert>
      ) : exposures === undefined ? <Text size="xs" role="status">正在读取模型透出状态…</Text> : null}
      <div className="provider-model-catalog-list" aria-label={provider.name + " 支持的模型列表"}>
        {rows.map((model) => {
          const ordinary = catalogModelKind(model.upstreamModelId) === "chat";
          const exposure = exposures?.find((candidate) => candidate.upstreamModelId === model.upstreamModelId);
          const pending = modelExposurePending(provider.id, model.upstreamModelId);
          const operationError = modelExposureError(provider.id, model.upstreamModelId);
          const exposed = exposure?.exposed ?? false;
          const state = exposure?.imageProbeState ?? "unknown";
          const referencedBy = exposure?.referencedBy ?? [];
          const missing = !discoveredIds.has(model.upstreamModelId);
          const disabledReason = error
            ? error
            : exposures === undefined
              ? "正在读取模型透出状态。"
              : pending
                ? "正在保存或检测这个模型。"
                : exposed && referencedBy.length > 0
                  ? `请先从${referencedBy.join("、")}移除此模型，再取消透出。`
                  : !exposed && !provider.configured
                    ? "请先补全此渠道的 API Key。"
                    : !exposed && !wireApi
                      ? "请先更新渠道，确认使用的协议。"
                      : undefined;
          const probeReason = !exposed
            ? "请先勾选透出模型。"
            : pending
              ? "正在检测这个模型。"
              : error
                ? error
                : !provider.configured || !wireApi
                  ? "请先补全渠道凭据并确认协议。"
                  : undefined;
          return (
            <div className="provider-model-catalog-row" key={model.upstreamModelId}>
              <strong>{model.displayName || model.upstreamModelId}</strong>
              <code>{model.upstreamModelId}</code>
              <small>{catalogModelPurpose(model.upstreamModelId)}</small>
              {missing ? <small>此模型仍有已保存的设置，当前目录未列出。</small> : null}
              {model.ownedBy && <small>{model.ownedBy}</small>}
              {ordinary ? (
                <>
                  <Group justify="space-between" wrap="wrap" gap="xs">
                    <Tooltip label={disabledReason} disabled={!disabledReason} multiline maw={340}>
                      <span>
                        <Checkbox
                          label="透出"
                          aria-label={`透出 ${model.upstreamModelId}`}
                          checked={exposed}
                          disabled={Boolean(disabledReason)}
                          onChange={(event) => void changeModelExposure(provider.id, model.upstreamModelId, event.currentTarget.checked)}
                        />
                      </span>
                    </Tooltip>
                    <Group gap="xs" wrap="nowrap">
                      <Text size="xs" c={pending ? "dimmed" : state === "failed" ? "red" : state === "supported" ? "teal" : "dimmed"} role="status">
                        {pending ? "正在保存并检测…" : imageProbeLabel(state)}
                      </Text>
                      <HintedButton
                        type="button"
                        aria-label={`刷新图片探针 ${model.upstreamModelId}`}
                        title="刷新图片探针，会发送一次图片请求"
                        disabled={Boolean(probeReason)}
                        disabledReason={probeReason}
                        onClick={() => void refreshModelImageProbe(provider.id, model.upstreamModelId)}
                      >
                        <RefreshCw size={14} aria-hidden="true" />
                      </HintedButton>
                    </Group>
                  </Group>
                  {exposure?.imageProbeCheckedAt ? <small>图片检测于 {formatTime(exposure.imageProbeCheckedAt)}</small> : null}
                  {exposure?.imageProbeErrorCode ? <Text size="xs" c="dimmed">{imageProbeErrorHint(exposure.imageProbeErrorCode)}</Text> : null}
                  {operationError ? <Text size="xs" c="red" role="alert">{operationError}</Text> : null}
                </>
              ) : null}
            </div>
          );
        })}
      </div>
    </Stack>
  );
}
