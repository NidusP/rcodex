import type { ModelExposureSummary } from "./model-exposure";
import {
  isWorkspaceSelectableModelId,
  looksLikeGptModelId,
} from "./components/model-labels";

export function channelProtocolConfirmed(
  wireApi: "responses" | "chat_completions" | null | undefined,
) {
  return wireApi === "responses" || wireApi === "chat_completions";
}

export function exposedWorkspaceModels(
  exposures: readonly ModelExposureSummary[] | undefined,
) {
  return (exposures ?? []).filter(
    (model) =>
      model.exposed && isWorkspaceSelectableModelId(model.upstreamModelId),
  );
}

export function hasExposedGptWorkspaceModel(
  exposures: readonly ModelExposureSummary[] | undefined,
) {
  return exposedWorkspaceModels(exposures).some((model) =>
    looksLikeGptModelId(model.upstreamModelId, model.upstreamModelId),
  );
}

/** 添加渠道：已透出对话模型，且协议已确认或至少透出 GPT 族。 */
export function channelCanAddToWorkspace(input: {
  configured: boolean;
  wireApi: "responses" | "chat_completions" | null | undefined;
  exposures: readonly ModelExposureSummary[] | undefined;
  exposureError: string | undefined;
  exposuresLoaded: boolean;
}) {
  if (!input.configured || input.exposureError || !input.exposuresLoaded) {
    return false;
  }
  if (exposedWorkspaceModels(input.exposures).length === 0) {
    return false;
  }
  return (
    channelProtocolConfirmed(input.wireApi) ||
    hasExposedGptWorkspaceModel(input.exposures)
  );
}

/** 勾选工作区模型：GPT 透出即可；非 GPT 仍须渠道协议已确认。 */
export function workspaceModelCanJoin(
  wireApi: "responses" | "chat_completions" | null | undefined,
  upstreamModelId: string,
) {
  if (looksLikeGptModelId(upstreamModelId, upstreamModelId)) {
    return true;
  }
  return channelProtocolConfirmed(wireApi);
}