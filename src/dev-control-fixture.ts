import { mockIPC } from "@tauri-apps/api/mocks";
import type { ModelExposureSummary } from "./model-exposure";
import { compareWorkspaceOrder } from "./workspace-order";

const now = Math.floor(Date.now() / 1000);

const providers = [
  { id: "provider-primary", name: "Primary API", baseUrl: "https://api.openai.com/v1", isDefault: true, configured: true, transportMode: "gateway_passthrough", currentRevisionId: "provider-revision-primary" },
  { id: "provider-relay", name: "Team Relay", baseUrl: "https://relay.example.test/v1", isDefault: false, configured: true, transportMode: "gateway_passthrough", currentRevisionId: "provider-revision-relay" },
  { id: "provider-spare", name: "Spare Relay", baseUrl: "https://spare.example.test/v1", isDefault: false, configured: true, transportMode: "gateway_passthrough", currentRevisionId: "provider-revision-spare" },
];

const compositions = [{
  launchPresetId: "preset-managed-app",
  apiKeyProviderIds: ["provider-relay", "provider-primary"],
  models: [
    { routeIdentityId: "route-relay", sourceKind: "api_provider", sourceId: "provider-relay", upstreamModelId: "grok-code-fast-1", publicModelId: "team-relay-grok-code-fast-1", displayName: "Grok Code Fast" },
  ],
  autoReviewPublicModelId: null as string | null,
  state: "ready",
}];

const snapshot = {
  providers,
  accounts: [
    { id: "account-personal", maskedIdentity: "p***l@example.test", workspaceLabel: "个人测试账号", planLabel: "Plus", currentCredentialRevisionId: "official-revision-personal", authState: "authorized", authCheckedAt: now - 180, authErrorCode: null, codexVersion: "codex-cli 0.147.0-alpha.6.5", schemaFingerprint: "fnv1a64:fixture-personal", quota: { primaryUsedPercent: 24, primaryWindowMinutes: 300, primaryResetsAt: now + 7200, secondaryUsedPercent: 41, secondaryWindowMinutes: 10080, secondaryResetsAt: now + 172800, refreshedAt: now - 180, expiresAt: now + 120, errorCode: null }, lockedByInstances: [] },
    { id: "account-work", maskedIdentity: "w**k@example.test", workspaceLabel: null, planLabel: "Team", currentCredentialRevisionId: "official-revision-work", authState: "needs_reauth", authCheckedAt: now - 3600, authErrorCode: "official_account_needs_reauth", codexVersion: "codex-cli 0.147.0-alpha.6.5", schemaFingerprint: "fnv1a64:fixture-work", quota: null, lockedByInstances: [] },
  ],
  presets: [
    { id: "preset-managed-app", entryKind: "managed_app", name: "隔离空间 A", credentialKind: "api_key", apiKeyProviderId: "provider-relay", gatewayPolicy: "gateway_passthrough", version: 1, isPreferred: false, lockedByInstances: [], loginProviderId: "provider-relay" },
  ],
  instances: [
    { id: "instance-api", launchTarget: "managed_app", launchConfigurationRevisionId: "launch-revision-api", launchPresetId: "preset-api", credentialKind: "api_key", credentialRevisionId: "provider-revision-primary", workspacePath: "/workspace/project", state: "running", supervisorPid: 2401, pid: 2402, launchedAt: now - 420, exitedAt: null, exitCode: null },
    { id: "instance-old", launchTarget: "cli", launchConfigurationRevisionId: "launch-revision-old", launchPresetId: "preset-personal", credentialKind: "official", credentialRevisionId: "official-revision-personal", workspacePath: "/workspace/archive", state: "exited", supervisorPid: 2201, pid: 2202, launchedAt: now - 7200, exitedAt: now - 3600, exitCode: 0 },
  ],
  compatibility: { controlPlaneAvailable: true, officialAccountsAvailable: false, managedAppAvailable: true, officialHybridAvailable: false },
};

const emptyUsage = {
  today: { requests: 0, successes: 0, inputTokens: null, outputTokens: null, estimatedCostUsd: null },
  week: { requests: 0, successes: 0, inputTokens: null, outputTokens: null, estimatedCostUsd: null },
  month: { requests: 0, successes: 0, inputTokens: null, outputTokens: null, estimatedCostUsd: null },
  lastRequestAt: null,
  upstreamBalance: { status: "unsupported", available: null, granted: null, used: null, unlimited: false, currency: null, unit: "credits", source: "fixture", refreshedAt: null, expiresAt: null, error: null },
};

const instanceDiagnostics = [{ id: 1, instanceId: "instance-api", sessionId: "sha256:session-fixture", createdAt: now - 120, route: "/v1/responses", stream: true, wireApi: "responses", publicModelId: "managed-model", upstreamModelId: "upstream-model", providerId: "provider-relay", providerRevisionId: "provider-revision-relay", httpStatus: 200, responseContentType: "text/event-stream", firstByteMs: 180, totalLatencyMs: 2400, outcome: "success", errorCode: null, inputLength: 128, outputLength: 640, inputHash: "sha256:input-fixture", outputHash: "sha256:output-fixture", toolCallTypes: ["function"], toolResultHashes: [] }];
const conversationSummaries = [{ id: 1, instanceId: "instance-api", sessionId: "sha256:session-fixture", createdAt: now - 120, inputLength: 128, outputLength: 640, inputHash: "sha256:input-fixture", outputHash: "sha256:output-fixture", toolCallTypes: ["function"], toolResultHashes: [] }];

export function installControlFixture() {
  const search = new URLSearchParams(window.location.search);
  let emptyWorkspace = search.get("empty-workspace") === "1";
  let gatewayUnavailable = search.get("gateway-unavailable") === "1";
  const workspaceFixtureKey = "rcodex.fixture.workspaces";
  const workspaceFixture = search.get("workspace-fixture") === "1";
  const savedWorkspaces = workspaceFixture ? window.localStorage.getItem(workspaceFixtureKey) : null;
  if (savedWorkspaces) {
    const saved = JSON.parse(savedWorkspaces) as { presets: typeof snapshot.presets; compositions: typeof compositions; instances: typeof snapshot.instances };
    snapshot.presets.splice(0, snapshot.presets.length, ...saved.presets);
    compositions.splice(0, compositions.length, ...saved.compositions);
    snapshot.instances.splice(0, snapshot.instances.length, ...saved.instances);
    emptyWorkspace = snapshot.presets.length === 0;
  } else if (emptyWorkspace || search.get("workspace-empty") === "1") {
    snapshot.presets.splice(0);
    compositions.splice(0);
    emptyWorkspace = true;
  } else if (search.get("multiple-workspaces") === "1") {
    const original = snapshot.presets[0];
    original.isPreferred = true;
    const second = { ...original, id: "preset-managed-app-b", name: "隔离空间 B", isPreferred: false };
    const third = { ...original, id: "preset-managed-app-c", name: "隔离空间 C", isPreferred: false };
    snapshot.presets.splice(0, snapshot.presets.length, third, original, second);
    compositions.push({ ...structuredClone(compositions[0]), launchPresetId: second.id });
    compositions.push({ ...structuredClone(compositions[0]), launchPresetId: third.id });
  }
  if (search.get("workspace-active") === "1" && snapshot.presets.length > 0) {
    snapshot.instances.push({ ...snapshot.instances[0], id: "instance-workspace-active", launchPresetId: snapshot.presets.find((preset) => preset.name === "隔离空间 A")?.id ?? snapshot.presets[0].id, launchTarget: "cli", state: "orphaned" });
  }
  const persistWorkspaces = () => {
    if (workspaceFixture) window.localStorage.setItem(workspaceFixtureKey, JSON.stringify({ presets: snapshot.presets, compositions, instances: snapshot.instances }));
  };
  persistWorkspaces();
  let workspaceDeleteCalls = 0;
  let archiveEnabled = false;
  let archiveRequests = [{ requestId: "db580361-c282-46aa-ae59-b4a0c43a5929", scopeId: "default", conversationKey: "b47ee4d5-ce72-40e2-9f74-54da80401918", createdAt: now - 90, stages: 4, partial: false, captureStatus: "complete", correlationStatus: "verified" }, { requestId: "1306c315-7c99-4282-a43b-ac99a5585fad", scopeId: "default", conversationKey: null, createdAt: now - 30, stages: 2, partial: true, captureStatus: "partial", correlationStatus: "pending" }];
  const archiveBodies: Record<string, string> = { ingress: '{"model":"gpt-5","input":[{"role":"user","content":"你好"}],"stream":true}', "upstream-request": '{"model":"gpt-5-mini","input":[{"role":"user","content":"你好"}],"stream":true}', "upstream-response": 'data: {"type":"response.completed"}\n\n', "downstream-response": 'data: {"type":"response.completed"}\n\n' };
  const fixtureModels = [
    { upstreamModelId: "grok-code-fast-1", displayName: "Grok Code Fast", ownedBy: "xai" },
    { upstreamModelId: "vision-test-model", displayName: "Vision Test Model", ownedBy: "fixture" },
    { upstreamModelId: "text-only-test-model", displayName: "Text Only Test Model", ownedBy: "fixture" },
    { upstreamModelId: "grok-imagine-image-2.0", displayName: "Grok Image Generation", ownedBy: "xai" },
    { upstreamModelId: "codex-auto-review", displayName: "Codex Auto Review", ownedBy: "fixture" },
  ];
  const exposureRows = new Map<string, ModelExposureSummary>();
  const probeCache = new Set<string>();
  let imageProbeCalls = 0;
  for (const provider of providers) {
    for (const model of fixtureModels.filter((item) => !["grok-imagine-image-2.0", "codex-auto-review"].includes(item.upstreamModelId))) {
      exposureRows.set(JSON.stringify([provider.id, model.upstreamModelId]), {
        providerId: provider.id,
        upstreamModelId: model.upstreamModelId,
        exposed: search.get("no-exposed-models") !== "1" && provider.id === "provider-relay" && model.upstreamModelId === "grok-code-fast-1",
        imageProbeState: "unknown",
        imageProbeCheckedAt: null,
        imageProbeErrorCode: null,
        referencedBy: [],
      });
    }
  }
  const modelReferences = (providerId: string, modelId: string) => emptyWorkspace ? [] : compositions.flatMap((composition) => {
    if (!composition.models.some((model) => model.sourceId === providerId && model.upstreamModelId === modelId)) return [];
    const preset = snapshot.presets.find((candidate) => candidate.id === composition.launchPresetId);
    return preset ? [preset.name] : [];
  });
  const exposureSummary = (row: ModelExposureSummary) => ({ ...row, referencedBy: modelReferences(row.providerId, row.upstreamModelId) });
  const probeFixtureImage = (row: ModelExposureSummary) => {
    imageProbeCalls += 1;
    document.documentElement.dataset.fixtureImageProbeCalls = String(imageProbeCalls);
    const result = search.get("model-probe") ?? (row.upstreamModelId === "text-only-test-model" ? "unsupported" : "supported");
    return new Promise<ModelExposureSummary>((resolve) => window.setTimeout(() => {
      row.imageProbeState = result === "failed" ? "failed" : result === "unsupported" ? "unsupported" : "supported";
      row.imageProbeErrorCode = result === "failed" ? "image_probe_timeout" : result === "unsupported" ? "unsupported_images" : null;
      row.imageProbeCheckedAt = Math.floor(Date.now() / 1000);
      probeCache.add(JSON.stringify([row.providerId, row.upstreamModelId]));
      resolve(exposureSummary(row));
    }, 300));
  };
  mockIPC((command, payload) => {
    switch (command) {
      case "list_provider_model_exposures": {
        if (search.get("exposure-load-error") === "1") throw "fixture_model_exposure_load_failed";
        const providerId = (payload as { providerId: string }).providerId;
        return [...exposureRows.values()].filter((model) => model.providerId === providerId).map(exposureSummary);
      }
      case "set_provider_model_exposure": {
        const input = payload as { providerId: string; upstreamModelId: string; exposed: boolean };
        const key = JSON.stringify([input.providerId, input.upstreamModelId]);
        const model = exposureRows.get(key);
        if (!model) throw "model_exposure_not_allowed";
        const references = modelReferences(input.providerId, input.upstreamModelId);
        if (!input.exposed && references.length > 0) throw `model_exposure_in_use:${references.join("、")}`;
        const changed = model.exposed !== input.exposed;
        model.exposed = input.exposed;
        return changed && input.exposed && !probeCache.has(key) ? probeFixtureImage(model) : exposureSummary(model);
      }
      case "refresh_model_image_probe": {
        const input = payload as { providerId: string; upstreamModelId: string };
        const model = exposureRows.get(JSON.stringify([input.providerId, input.upstreamModelId]));
        if (!model?.exposed) throw "model_not_exposed";
        return probeFixtureImage(model);
      }
      case "conversation_archive_status": return { enabled: archiveEnabled, root: "/Users/fixture/Library/Application Support/io.rcodex/conversation-history", requestCount: archiveRequests.length, stageCount: archiveRequests.reduce((sum, request) => sum + request.stages, 0), storedBytes: 512 };
      case "conversation_archive_set_policy": archiveEnabled = Boolean((payload as { enabled: boolean }).enabled); return { enabled: archiveEnabled, root: "/Users/fixture/Library/Application Support/io.rcodex/conversation-history", requestCount: archiveRequests.length, stageCount: archiveRequests.reduce((sum, request) => sum + request.stages, 0), storedBytes: 512 };
      case "conversation_archive_list": return archiveRequests;
      case "conversation_archive_detail": {
        const request = archiveRequests.find((entry) => entry.requestId === (payload as { requestId: string }).requestId);
        return request ? { request, stages: Object.entries(archiveBodies).slice(0, request.stages).map(([stage, body]) => ({ stage, observedBytes: body.length, storedBytes: body.length, partial: false })), metadata: { outcome: "success", publicModel: "gpt-5", upstreamModel: "gpt-5-mini" } } : null;
      }
      case "conversation_archive_body": {
        const input = payload as { stage: string; offset: number; limit: number };
        const body = archiveBodies[input.stage] ?? "";
        const text = body.slice(input.offset, input.offset + input.limit);
        return { text, offset: input.offset, nextOffset: input.offset + text.length, totalBytes: body.length, truncated: false };
      }
      case "conversation_archive_clear": archiveRequests = []; return null;
      case "gateway_status": return gatewayUnavailable
        ? { state: "unavailable", baseUrl: null, bindHost: "127.0.0.1", port: 43123, activeBindings: 0, inFlightRequests: 0 }
        : { state: "stopped", baseUrl: null, bindHost: "127.0.0.1", port: null, activeBindings: 0, inFlightRequests: 0 };
      case "restart_unavailable_gateway": gatewayUnavailable = false; return { state: "serving", baseUrl: "http://127.0.0.1:43123/v1", bindHost: "127.0.0.1", port: 43123, activeBindings: 0, inFlightRequests: 0 };
      case "codex_installation_status": return { installed: true, executablePath: "/usr/local/bin/codex", version: "codex-cli 0.150.0", executableUpdatedAt: now - 120, appPath: "/Applications/Codex.app", appVersion: "1.0.0", appUpdatedAt: now - 120, auth: { state: "not_detected" }, error: null };
      case "list_api_key_providers": return providers;
      case "get_codex_control_snapshot": return emptyWorkspace ? { ...snapshot, presets: [] } : {
        ...snapshot,
        // Deliberately emulate the old preferred-first backend order so the
        // defensive fixed-slot UI sort is exercised on every refresh.
        presets: snapshot.presets.slice().sort((left, right) => Number(right.isPreferred) - Number(left.isPreferred)),
      };
      case "delete_managed_workspace": {
        const input = (payload as { input: { launchPresetId: string; expectedVersion: number } }).input;
        workspaceDeleteCalls += 1;
        document.documentElement.dataset.fixtureWorkspaceDeleteCalls = String(workspaceDeleteCalls);
        const target = snapshot.presets.find((preset) => preset.id === input.launchPresetId);
        if (!target) throw "managed_preset_not_found";
        if (target.version !== input.expectedVersion) throw "version_conflict";
        if (snapshot.instances.some((instance) => instance.launchPresetId === target.id && ["launching", "running", "stopping", "orphaned", "degraded"].includes(instance.state))) throw "workspace_delete_active_instances";
        if (search.get("workspace-delete-error") === "1") throw "workspace_delete_failed:测试目录暂不可用";
        return new Promise((resolve) => window.setTimeout(() => {
          snapshot.presets.splice(snapshot.presets.indexOf(target), 1);
          for (let i = compositions.length - 1; i >= 0; i -= 1) if (compositions[i].launchPresetId === target.id) compositions.splice(i, 1);
          for (let i = snapshot.instances.length - 1; i >= 0; i -= 1) if (snapshot.instances[i].launchPresetId === target.id) snapshot.instances.splice(i, 1);
          // 与后端一致：删除非选中空间保持当前选中；删除选中的空间回落到剩余第一个槽位。
          const selected = snapshot.presets.find((preset) => preset.isPreferred) ?? snapshot.presets.slice().sort(compareWorkspaceOrder)[0];
          snapshot.presets.forEach((preset) => { preset.isPreferred = preset.id === selected?.id; });
          emptyWorkspace = snapshot.presets.length === 0;
          persistWorkspaces();
          resolve({ deletedPresetId: target.id, selectedPresetId: selected?.id ?? null });
        }, 250));
      }
      case "set_managed_login_provider": {
        const input = (payload as { input: { launchPresetId: string; providerId: string } }).input;
        const preset = snapshot.presets.find((item) => item.id === input.launchPresetId);
        if (!preset) throw "launch_preset_not_found";
        preset.loginProviderId = input.providerId;
        preset.version += 1;
        return { id: preset.id, version: preset.version };
      }
      case "list_importable_session_projects": return search.get("import-empty") === "1" ? [] : [{
        cwd: "/workspace/project", sessionCount: 1, latestTitle: "旧项目会话",
        sessions: [{ id: "fixture-thread", title: "旧项目会话", provider: "openai", createdAt: now - 600 }],
      }];
      case "import_sessions_from_default": {
        if (search.get("import-error") === "busy") throw "session_import_busy";
        return { copied: 1, skipped: 0 };
      }
      case "launch_codex": return { id: "fixture-launch", launchPresetId: "preset-managed-app", launchTarget: "managed_app", state: "running", credentialKind: "managed_route_set" };
      case "create_managed_launch_preset": {
        const input = (payload && !Array.isArray(payload) && typeof payload === "object" && "input" in payload ? payload.input : {}) as {
          entryKind?: string;
          name?: string;
          createNew?: boolean;
          launchPresetId?: string;
          apiKeyProviderIds?: string[];
          models?: Array<Record<string, string>>;
          autoReviewPublicModelId?: string | null;
          loginProviderId?: string | null;
        };
        const apply = (preset: (typeof snapshot.presets)[number], composition: (typeof compositions)[number]) => {
          if (preset.entryKind !== "managed_app") preset.name = input.name ?? preset.name;
          preset.gatewayPolicy = "gateway_passthrough";
          preset.version += 1;
          composition.apiKeyProviderIds = input.apiKeyProviderIds ?? [];
          composition.models = (input.models ?? []).map((model, index) => ({
            routeIdentityId: `route-fixture-${index}`,
            sourceKind: model.sourceKind,
            sourceId: model.sourceId,
            upstreamModelId: model.upstreamModelId,
            publicModelId: model.publicModelId,
            displayName: model.displayName,
          }));
          composition.autoReviewPublicModelId = input.autoReviewPublicModelId ?? null;
          composition.state = composition.models.length > 0 ? "ready" : "needs_model_selection";
          emptyWorkspace = false;
          persistWorkspaces();
          return { id: preset.id, version: preset.version };
        };
        if (input.createNew) {
          if (snapshot.presets.filter((item) => item.entryKind === "managed_app").length >= 5) throw "managed_app_limit_reached";
          const id = `preset-managed-app-${crypto.randomUUID()}`;
          const slotName = ["隔离空间 A", "隔离空间 B", "隔离空间 C", "隔离空间 D", "隔离空间 E"].find((name) => !snapshot.presets.some((preset) => preset.name === name)) ?? "隔离空间";
          snapshot.presets.forEach((item) => { item.isPreferred = false; });
          const preset = {
            id,
            entryKind: input.entryKind ?? "managed_app",
            name: slotName,
            credentialKind: "api_key" as const,
            apiKeyProviderId: input.apiKeyProviderIds?.[0],
            gatewayPolicy: "gateway_passthrough",
            version: 0,
            isPreferred: true,
            lockedByInstances: [],
            loginProviderId: input.loginProviderId ?? input.apiKeyProviderIds?.[0] ?? null,
          };
          snapshot.presets.push(preset as (typeof snapshot.presets)[number]);
          const composition: (typeof compositions)[number] = {
            launchPresetId: id,
            apiKeyProviderIds: [],
            models: [],
            autoReviewPublicModelId: null,
            state: "needs_model_selection",
          };
          compositions.push(composition);
          return apply(preset as (typeof snapshot.presets)[number], composition);
        }
        let preset = snapshot.presets.find((candidate) => candidate.id === input.launchPresetId)
          ?? snapshot.presets.find((candidate) => candidate.isPreferred)
          ?? snapshot.presets.find((candidate) => candidate.entryKind === input.entryKind);
        if (!preset && input.entryKind === "managed_app") {
          const id = "preset-managed-app-initial";
          preset = ({
            id,
            entryKind: "managed_app",
            name: "隔离空间 A",
            credentialKind: "none" as const,
            apiKeyProviderId: undefined,
            gatewayPolicy: null,
            version: 0,
            isPreferred: true,
            lockedByInstances: [],
            loginProviderId: null,
          } as unknown as (typeof snapshot.presets)[number]);
          snapshot.presets.push(preset as (typeof snapshot.presets)[number]);
          compositions.push({
            launchPresetId: id,
            apiKeyProviderIds: [],
            models: [],
            autoReviewPublicModelId: null,
            state: "needs_model_selection",
          });
          emptyWorkspace = false;
        }
        const composition = compositions.find((candidate) => candidate.launchPresetId === preset?.id);
        if (preset && composition) return apply(preset, composition);
        return null;
      }
      case "select_managed_launch_preset": {
        const input = (payload && !Array.isArray(payload) && typeof payload === "object" && "input" in payload ? payload.input : {}) as {
          launchPresetId?: string;
        };
        const preset = snapshot.presets.find((candidate) => candidate.id === input.launchPresetId);
        if (!preset) throw "launch_preset_not_found";
        if (search.get("workspace-selection-error") === "1") throw "version_conflict";
        snapshot.presets.forEach((item) => { item.isPreferred = item.id === preset.id; });
        persistWorkspaces();
        return { id: preset.id, version: preset.version };
      }
      case "rename_launch_preset": {
        const args = (payload && !Array.isArray(payload) && typeof payload === "object" ? payload : {}) as {
          launchPresetId?: string;
          input?: { expectedVersion?: number; name?: string };
        };
        const preset = snapshot.presets.find((candidate) => candidate.id === args.launchPresetId);
        if (!preset) throw "launch_preset_not_found";
        if (preset.entryKind === "managed_app") throw "managed_app_name_fixed";
        if (preset.version !== args.input?.expectedVersion) throw "version_conflict";
        const name = (args.input?.name ?? "").trim();
        if (!name || new TextEncoder().encode(name).length > 80) throw "invalid_launch_preset_name";
        if (preset.name !== name) {
          preset.name = name;
          preset.version += 1;
        }
        return { id: preset.id, name: preset.name, version: preset.version, isPreferred: preset.isPreferred };
      }
      case "get_managed_launch_preset_compositions": return compositions;
      case "get_codex_special_model_roles": return [];
      case "reconcile_codex_instance": {
        const instanceId = (payload as { instanceId: string }).instanceId;
        return snapshot.instances.find((instance) => instance.id === instanceId) ?? null;
      }
      case "get_provider_usage_summary": {
        const providerId = (payload as { providerId?: string }).providerId;
        if (providerId === "provider-relay") return { ...emptyUsage, today: { ...emptyUsage.today, requests: 3, successes: 2, inputTokens: 120, outputTokens: 80 }, lastRequestAt: now - 90 };
        return emptyUsage;
      }
      case "test_api_key_provider":
      case "get_provider_connectivity_report": {
        const providerId = (payload as { providerId?: string }).providerId ?? "";
        if (providerId === "provider-primary") return { providerId, checkedAt: now - 50, status: "unauthorized", model: null, models: [], modelsCheck: "failed", textCheck: "not_run", errorCode: "unauthorized", wireApi: null };
        return { providerId, checkedAt: now - 40, status: "ready", model: "grok-code-fast-1", models: fixtureModels, modelsCheck: "ready", textCheck: "ready", errorCode: null, wireApi: "responses" };
      }
      case "gateway_recent_diagnostics": return search.get("no-failure") === "1" ? [] : [{ id: 9, providerId: "provider-primary", createdAt: now - 30, source: "probe", route: "/models", model: null, transportMode: "gateway_passthrough", stream: false, outcome: "failed", httpStatus: 401, latencyMs: 120, errorCode: "unauthorized" }];
      case "recent_gateway_requests": return search.get("no-failure") === "1" ? [] : [{ id: 4, instanceId: "instance-api", sessionId: null, createdAt: now - 20, route: "/v1/responses", stream: false, wireApi: "responses", publicModelId: "team-relay-grok-code-fast-1", upstreamModelId: "grok-code-fast-1", providerId: "provider-relay", providerRevisionId: "provider-revision-relay", httpStatus: 502, responseContentType: null, firstByteMs: null, totalLatencyMs: 800, outcome: "failed", errorCode: "upstream_unavailable", errorDetail: "relay.example.test/v1", inputLength: 20, outputLength: 0, inputTokens: null, outputTokens: null, totalTokens: null }];
      case "check_codex_updates": return { state: "up_to_date", checkedAt: now, currentVersion: "0.150.0", latestVersion: "0.150.0" };
      case "instance_request_diagnostics": return instanceDiagnostics;
      case "instance_conversation_summaries": return conversationSummaries;
      case "clear_instance_diagnostics": return 2;
      case "start_codex_account_authorization": {
        const input = (payload && !Array.isArray(payload) && typeof payload === "object" && "input" in payload ? payload.input : {}) as { mode?: string };
        return input.mode === "device_code"
          ? { attemptId: "authorization-fixture", accountId: "account-work", credentialRevisionId: "official-revision-pending", state: "awaiting_user", prompt: { type: "device_code", verificationUrl: "https://auth.example.test/device", userCode: "ABCD-EFGH" } }
          : { attemptId: "authorization-fixture", accountId: "account-work", credentialRevisionId: "official-revision-pending", state: "awaiting_user", prompt: { type: "browser", authUrl: "https://auth.example.test/fixture" } };
      }
      case "poll_codex_account_authorization": return { attemptId: "authorization-fixture", accountId: "account-work", credentialRevisionId: "official-revision-pending", state: "awaiting_user", errorCode: null };
      case "cancel_codex_account_authorization": return { attemptId: "authorization-fixture", accountId: "account-work", credentialRevisionId: "official-revision-pending", state: "cancelled", errorCode: null };
      case "open_codex_account_authorization_prompt": return null;
      default: return null;
    }
  }, { shouldMockEvents: true });
}
