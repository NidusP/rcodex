// Pure copy of LaunchPresetDrawer discoveredOptions and toggleDiscoveredModel.
// Callers remove Codex system-role models before passing sources in.

export type CatalogModel = {
  upstreamModelId: string;
  displayName: string;
  ownedBy: string | null;
};

export type CatalogSource = {
  providerId: string;
  providerName: string;
  models: CatalogModel[];
};

export type WorkspaceModelSelection = {
  sourceKind: "official" | "api_provider";
  sourceId: string;
  upstreamModelId: string;
  publicModelId: string;
  displayName: string;
  contextWindow?: number;
  autoCompactTokenLimit?: number;
  reasoningLevels?: string[];
  defaultReasoningLevel?: string;
};

export type DiscoveredModelOption = {
  providerId: string;
  providerName: string;
  upstreamModelId: string;
  displayName: string;
  ownedBy: string | null;
  sourceKey: string;
  duplicate: boolean;
  publicModelId: string;
  displayLabel: string;
};

function defaultPublicModelId(modelId: string) {
  const normalized = modelId
    .toLowerCase()
    .replace(/[^a-z0-9._-]/g, "-")
    .replace(/-+/g, "-")
    .replace(/^-|-$/g, "");
  return normalized || "model";
}

function providerModelSlug(providerId: string) {
  const normalized = defaultPublicModelId(providerId);
  return normalized === "model"
    ? `provider-${providerId.slice(0, 8)}`
    : normalized.slice(0, 32);
}

function compareModelSortText(left: string, right: string) {
  return left.localeCompare(right, undefined, {
    numeric: true,
    sensitivity: "base",
  });
}

function modelDraftKey(sourceKey: string, upstreamModelId: string) {
  return `${sourceKey}\u0000${upstreamModelId}`;
}

function selectionSourceKey(model: WorkspaceModelSelection) {
  return `${model.sourceKind}:${model.sourceId}`;
}

function sourceIdentity(sourceKey: string): {
  sourceKind: WorkspaceModelSelection["sourceKind"];
  sourceId: string;
} {
  if (sourceKey.startsWith("api_provider:")) {
    return {
      sourceKind: "api_provider",
      sourceId: sourceKey.slice("api_provider:".length),
    };
  }
  if (sourceKey.startsWith("official:")) {
    return {
      sourceKind: "official",
      sourceId: sourceKey.slice("official:".length),
    };
  }
  throw new Error(`Unsupported model source key: ${sourceKey}`);
}

export function buildDiscoveredModelOptions(
  sources: CatalogSource[],
): DiscoveredModelOption[] {
  const discoveredModels = sources
    .flatMap((source) =>
      source.models.map((model) => ({
        upstreamModelId: model.upstreamModelId,
        displayName: model.displayName,
        ownedBy: model.ownedBy,
        sourceKey: `api_provider:${source.providerId}`,
        providerId: source.providerId,
        providerName: source.providerName,
      })),
    )
    .sort(
      (left, right) =>
        compareModelSortText(left.providerName, right.providerName) ||
        compareModelSortText(
          left.displayName || left.upstreamModelId,
          right.displayName || right.upstreamModelId,
        ) ||
        compareModelSortText(left.upstreamModelId, right.upstreamModelId),
    );

  const discoveredModelCounts = new Map<string, number>();
  discoveredModels.forEach((model) => {
    const keys = new Set(
      [
        model.upstreamModelId.trim().toLowerCase(),
        model.displayName.trim().toLowerCase(),
      ].filter(Boolean),
    );
    keys.forEach((key) =>
      discoveredModelCounts.set(key, (discoveredModelCounts.get(key) ?? 0) + 1),
    );
  });

  const usedPublicIds = new Set<string>();
  return discoveredModels.map((model) => {
    const duplicate = [model.upstreamModelId, model.displayName].some(
      (value) =>
        value.trim() &&
        (discoveredModelCounts.get(value.trim().toLowerCase()) ?? 0) > 1,
    );
    const baseId = defaultPublicModelId(model.upstreamModelId);
    const qualifiedId = `${providerModelSlug(model.providerId)}-${baseId}`;
    let publicModelId = duplicate ? qualifiedId : baseId;
    let suffix = 2;
    while (usedPublicIds.has(publicModelId))
      publicModelId = `${duplicate ? qualifiedId : baseId}-${suffix++}`;
    usedPublicIds.add(publicModelId);
    return {
      providerId: model.providerId,
      providerName: model.providerName,
      upstreamModelId: model.upstreamModelId,
      displayName: model.displayName,
      ownedBy: model.ownedBy,
      sourceKey: model.sourceKey,
      duplicate,
      publicModelId,
      displayLabel: duplicate
        ? `${model.providerName}/${model.displayName || model.upstreamModelId}`
        : model.displayName || model.upstreamModelId,
    };
  });
}

export function toggleWorkspaceModel(
  current: WorkspaceModelSelection[],
  options: DiscoveredModelOption[],
  target: DiscoveredModelOption,
): WorkspaceModelSelection[] {
  const targetKey = modelDraftKey(target.sourceKey, target.upstreamModelId);
  const selected = current.some(
    (model) =>
      modelDraftKey(selectionSourceKey(model), model.upstreamModelId) ===
      targetKey,
  );
  if (selected) {
    return current.filter(
      (model) =>
        modelDraftKey(selectionSourceKey(model), model.upstreamModelId) !==
        targetKey,
    );
  }

  const rewritten = current.map((existing) => {
    const existingSourceKey = selectionSourceKey(existing);
    if (
      existingSourceKey === target.sourceKey ||
      existing.upstreamModelId !== target.upstreamModelId
    ) {
      return existing;
    }
    const original = options.find(
      (candidate) =>
        modelDraftKey(candidate.sourceKey, candidate.upstreamModelId) ===
        modelDraftKey(existingSourceKey, existing.upstreamModelId),
    );
    const baseId = defaultPublicModelId(existing.upstreamModelId);
    if (
      original &&
      existing.displayName ===
        (original.displayName || original.upstreamModelId) &&
      existing.publicModelId === baseId
    ) {
      return {
        ...existing,
        displayName: `${original.providerName}/${original.displayName || original.upstreamModelId}`,
        publicModelId: `${providerModelSlug(original.providerId)}-${baseId}`,
      };
    }
    return existing;
  });

  const identity = sourceIdentity(target.sourceKey);
  return [
    ...rewritten,
    {
      sourceKind: identity.sourceKind,
      sourceId: identity.sourceId,
      upstreamModelId: target.upstreamModelId,
      publicModelId: target.publicModelId,
      displayName: target.displayLabel,
    },
  ];
}
