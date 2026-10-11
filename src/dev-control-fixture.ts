import { mockIPC } from "@tauri-apps/api/mocks";

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
    { id: "preset-managed-app", entryKind: "managed_app", name: "R Codex", credentialKind: "api_key", apiKeyProviderId: "provider-relay", gatewayPolicy: "gateway_passthrough", version: 1, isPreferred: false, lockedByInstances: [], loginProviderId: "provider-relay" },
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
  let gatewayUnavailable = search.get("gateway-unavailable") === "1";
  let archiveEnabled = false;
  let archiveRequests = [{ requestId: "db580361-c282-46aa-ae59-b4a0c43a5929", scopeId: "default", conversationKey: "b47ee4d5-ce72-40e2-9f74-54da80401918", createdAt: now - 90, stages: 4, partial: false, captureStatus: "complete", correlationStatus: "verified" }, { requestId: "1306c315-7c99-4282-a43b-ac99a5585fad", scopeId: "default", conversationKey: null, createdAt: now - 30, stages: 2, partial: true, captureStatus: "partial", correlationStatus: "pending" }];
  const archiveBodies: Record<string, string> = { ingress: '{"model":"gpt-5","input":[{"role":"user","content":"你好"}],"stream":true}', "upstream-request": '{"model":"gpt-5-mini","input":[{"role":"user","content":"你好"}],"stream":true}', "upstream-response": 'data: {"type":"response.completed"}\n\n', "downstream-response": 'data: {"type":"response.completed"}\n\n' };
  mockIPC((command, payload) => {
    switch (command) {
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
      case "get_codex_control_snapshot": return snapshot;
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
        };
        const apply = (preset: (typeof snapshot.presets)[number], composition: (typeof compositions)[number]) => {
          preset.name = input.name ?? preset.name;
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
          return { id: preset.id, version: preset.version };
        };
        if (input.createNew) {
          const id = `preset-managed-app-${snapshot.presets.length + 1}`;
          snapshot.presets.forEach((item) => { item.isPreferred = false; });
          const preset = {
            id,
            entryKind: input.entryKind ?? "managed_app",
            name: input.name ?? "实例",
            credentialKind: "api_key" as const,
            apiKeyProviderId: input.apiKeyProviderIds?.[0],
            gatewayPolicy: "gateway_passthrough",
            version: 0,
            isPreferred: true,
            lockedByInstances: [],
            loginProviderId: input.apiKeyProviderIds?.[0] ?? null,
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
        const preset = snapshot.presets.find((candidate) => candidate.id === input.launchPresetId)
          ?? snapshot.presets.find((candidate) => candidate.isPreferred)
          ?? snapshot.presets.find((candidate) => candidate.entryKind === input.entryKind);
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
        snapshot.presets.forEach((item) => { item.isPreferred = item.id === preset.id; });
        return { id: preset.id, version: preset.version };
      }
      case "rename_launch_preset": {
        const args = (payload && !Array.isArray(payload) && typeof payload === "object" ? payload : {}) as {
          launchPresetId?: string;
          input?: { expectedVersion?: number; name?: string };
        };
        const preset = snapshot.presets.find((candidate) => candidate.id === args.launchPresetId);
        if (!preset) throw "launch_preset_not_found";
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
      case "get_provider_usage_summary": {
        const providerId = (payload as { providerId?: string }).providerId;
        if (providerId === "provider-relay") return { ...emptyUsage, today: { ...emptyUsage.today, requests: 3, successes: 2, inputTokens: 120, outputTokens: 80 }, lastRequestAt: now - 90 };
        return emptyUsage;
      }
      case "get_provider_connectivity_report": {
        const providerId = (payload as { providerId?: string }).providerId ?? "";
        if (providerId === "provider-primary") return { providerId, checkedAt: now - 50, status: "unauthorized", model: null, models: [], modelsCheck: "failed", textCheck: "not_run", errorCode: "unauthorized", wireApi: null };
        return { providerId, checkedAt: now - 40, status: "ready", model: "grok-code-fast-1", models: [{ upstreamModelId: "grok-code-fast-1", displayName: "Grok Code Fast", ownedBy: "xai" }], modelsCheck: "ready", textCheck: "ready", errorCode: null, wireApi: "responses" };
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
