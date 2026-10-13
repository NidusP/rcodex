import type {
  ApiKeyProviderSummary,
  ManagedPresetModel,
  OfficialAccountSummary,
} from "../codex-control";

export type DiscoveredModelRef = {
  upstreamModelId: string;
  displayName: string;
  ownedBy: string | null;
};

export function isInternalCodexModelId(modelId: string) {
  const normalized = modelId.trim().toLowerCase();
  if (normalized === "codex-auto-review" || normalized === "gpt-code-review")
    return true;
  if (!normalized.startsWith("codex-") && !normalized.startsWith("gpt-"))
    return false;
  return /(?:auto[-_]?review|code[-_]?review|auto[-_]?subagent|sub[-_]?agent)$/.test(
    normalized,
  );
}

export function visibleProviderModels<T extends { upstreamModelId: string }>(
  models: T[],
) {
  return models.filter((model) => !isInternalCodexModelId(model.upstreamModelId));
}

/** 探测结果里被现有规则隐藏的 Codex 系统角色模型数量。 */
export function hiddenCodexRoleModelCount<
  T extends { upstreamModelId: string },
>(models: readonly T[]) {
  return models.filter((model) => isInternalCodexModelId(model.upstreamModelId))
    .length;
}

/**
 * 给探测结果和模型勾选用的一句说明。数量为 0 时返回 null。
 * 系统角色用于自动审核或子代理等特殊调用，不进入 Codex 普通模型目录。
 */
export function hiddenCodexRoleModelNote(count: number) {
  if (count <= 0) return null;
  return (
    "已隐藏 " +
    count +
    " 个 Codex 系统角色模型，它们用于自动审核或子代理等特殊调用，不会出现在 Codex 模型目录，也不能作为普通模型加入工作区。"
  );
}

export type CatalogModelKind = "chat" | "image_generation" | "codex_role";

/**
 * 图像生成模型走单独的出图接口，不是可看图的对话模型。
 * 只认生图 ID（imagine、dall-e、gpt-image、imagen、以 image 加版本结尾）。
 * grok-4 这类能看图的聊天模型不要算进来。
 */
export function isImageGenerationModelId(modelId: string) {
  const normalized = modelId.trim().toLowerCase();
  if (!normalized) return false;
  if (normalized.includes("imagine")) return true;
  if (normalized.includes("dall-e") || normalized.includes("dalle")) return true;
  if (normalized.includes("gpt-image")) return true;
  if (/(?:^|[^a-z0-9])imagen(?:[^a-z]|[0-9]|$)/.test(normalized)) return true;
  return /(?:^|[^a-z0-9])image(?:-[0-9].*|$)/.test(normalized);
}

/** 探测结果里这个模型在目录和工作区勾选中分别扮演什么角色。 */
export function catalogModelKind(modelId: string): CatalogModelKind {
  if (isInternalCodexModelId(modelId)) return "codex_role";
  if (isImageGenerationModelId(modelId)) return "image_generation";
  return "chat";
}

export function isWorkspaceSelectableModelId(modelId: string) {
  return catalogModelKind(modelId) === "chat";
}

/** 工作区可以勾选、启动后进入 Codex 对话目录的模型。 */
export function selectableProviderModels<T extends { upstreamModelId: string }>(
  models: readonly T[],
) {
  return models.filter((model) =>
    isWorkspaceSelectableModelId(model.upstreamModelId),
  );
}

/** 模型目录里给单个模型的一句用途。 */
export function catalogModelPurpose(modelId: string) {
  const kind = catalogModelKind(modelId);
  if (kind === "codex_role") {
    return "Codex 系统角色，用于自动审核或子代理等特殊调用。不会出现在 Codex 模型目录，也不会显示在工作区勾选中。";
  }
  if (kind === "image_generation") {
    return "图像生成模型，走单独的生图接口，不是对话模型。不会显示在工作区勾选中，也不会进入 Codex 模型目录。";
  }
  return "对话模型。可以在工作区勾选，启动后显示在 Codex 模型目录。";
}

/** 探测结果里图像生成模型的一句汇总。没有这类模型时返回 null。 */
export function imageGenerationModelNote(names: readonly string[]) {
  const labels = names.map((name) => name.trim()).filter(Boolean);
  if (labels.length === 0) return null;
  const shown = labels.slice(0, 3).join("、");
  const extra = labels.length > 3 ? " 等 " + labels.length + " 个模型" : "";
  return shown + extra + " 是图像生成模型，用于单独出图，不会显示在工作区勾选中。";
}

export function protocolText(
  wireApi: "responses" | "chat_completions" | null | undefined,
) {
  if (wireApi === "responses") return "Responses";
  if (wireApi === "chat_completions") return "Chat Completions";
  if (wireApi === null) return "协议未确认";
  return "未检测";
}

export function providerHost(baseUrl: string) {
  try {
    return new URL(baseUrl).hostname;
  } catch {
    return "自定义上游";
  }
}

export function compareModelSortText(left: string, right: string) {
  return left.localeCompare(right, undefined, {
    numeric: true,
    sensitivity: "base",
  });
}

export function providerForModel(
  model: ManagedPresetModel,
  providers: ApiKeyProviderSummary[],
  accounts: OfficialAccountSummary[],
) {
  if (model.sourceKind === "api_provider")
    return providers.find((provider) => provider.id === model.sourceId);
  return accounts.find((account) => account.id === model.sourceId);
}

export function sourceLabelForModel(
  model: ManagedPresetModel,
  providers: ApiKeyProviderSummary[],
  accounts: OfficialAccountSummary[],
) {
  const source = providerForModel(model, providers, accounts);
  if (model.sourceKind === "api_provider")
    return (source as ApiKeyProviderSummary | undefined)?.name ?? "模型渠道";
  return (
    (source as OfficialAccountSummary | undefined)?.maskedIdentity ?? "官方账户"
  );
}

export function qualifiedModelLabel(
  model: ManagedPresetModel,
  providers: ApiKeyProviderSummary[],
  accounts: OfficialAccountSummary[],
) {
  const sourceLabel = sourceLabelForModel(model, providers, accounts);
  const prefixes = [`${sourceLabel}/`, `${sourceLabel} /`];
  let modelLabel = (model.displayName || model.upstreamModelId).trim();
  while (true) {
    const prefix = prefixes.find((candidate) => modelLabel.startsWith(candidate));
    if (!prefix) break;
    modelLabel = modelLabel.slice(prefix.length).trim();
  }
  return `${sourceLabel}/${modelLabel}`;
}

export function orderModelsByChannels<T extends { sourceId: string }>(
  models: T[],
  providerIds: string[],
) {
  const rank = new Map(providerIds.map((id, index) => [id, index]));
  return models
    .map((model, index) => ({ model, index }))
    .sort((left, right) => {
      const leftRank = rank.get(left.model.sourceId) ?? providerIds.length;
      const rightRank = rank.get(right.model.sourceId) ?? providerIds.length;
      return leftRank - rightRank || left.index - right.index;
    })
    .map((item) => item.model);
}

export function sortedManagedModels(
  models: ManagedPresetModel[],
  providerIds: string[],
) {
  return orderModelsByChannels(models, providerIds);
}

export const connectivityLabels = {
  ready: "可用",
  degraded: "不可用",
  unreachable: "无法连接",
  unauthorized: "认证失败",
} as const;

export type ConnectivityLabelStatus = keyof typeof connectivityLabels;
