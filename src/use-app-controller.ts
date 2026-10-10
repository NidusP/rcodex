import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { createElement, useEffect, useMemo, useRef, useState } from "react";
import { notifications } from "@mantine/notifications";
import {
  getCodexControlSnapshot,
  getCodexSpecialModelRoles,
  invokeControl,
  isActiveInstanceState,
  isApiKeyCredential,
  isKnownLaunchTarget,
  isManagedAppTarget,
  isOfficialCredential,
  runningWorkspaceRestartNotice,
  usesOfficialModel,
  toCommandError,
  type AuthorizationStarted,
  type AuthorizationStatus,
  type CodexCompatibility,
  type CodexInstallationStatus,
  type CodexUpdateStatus,
  type CommandError,
  type LaunchPresetSummary,
  type LaunchTarget,
  type ManagedPresetModel,
  type OfficialAccountSummary,
  type RuntimeInstanceSummary,
} from "./codex-control";
import { desktopChrome } from "./components/title-bar";
import { checkAppRelease, openAppReleasePage, type AppReleaseUpdate } from "./components/app-update";

import {
  buildDiscoveredModelOptions,
  toggleWorkspaceModel,
} from "./workspace-model-selection";
import {
  compareModelSortText,
  connectivityLabels,
  isImageGenerationModelId,
  isInternalCodexModelId,
  isWorkspaceSelectableModelId,
  orderModelsByChannels,
  protocolText,
  providerHost,
  qualifiedModelLabel,
  selectableProviderModels,
  sortedManagedModels,
} from "./components/model-labels";
import type {
  DraftChannelProbe,
  ProviderChannelDraft,
} from "./components/model-utility";
import {
  OFFICIAL_AUTH_ENABLED,
  defaultReasoningLevel,
  getInitialThemeMode,
  modelSelectionPayload,
  optionalWindowValue,
  prefersDarkTheme,
  stoppedGateway,
  accentStorageKey,
  getInitialAccent,
  themeStorageKey,
  type ActiveAuthorization,
  type AuthorizationDraft,
  type ConversationSummary,
  type DiagnosticEventModel,
  type GatewayDiagnosticEvent,
  type GatewayStatus,
  type InstanceRequestDiagnostic,
  type Provider,
  type ProviderConnectivityReport,
  type ProviderUsageSummary,
  type RequestEventModel,
  type ThemeMode,
  type UtilityDrawerView,
} from "./app-model";
import type { SpecialModelRoleSummary } from "./codex-control";
import type { UpstreamBalance } from "./app-model";

function showAppNotice(
  message: string,
  options?: {
    error?: boolean;
    action?: { label: string; onClick: () => void };
  },
) {
  if (options?.error) return;
  notifications.clean();
  notifications.show({
    message: options?.action
      ? createElement(
          "span",
          { className: "app-notice-message" },
          message,
          createElement(
            "button",
            {
              type: "button",
              className: "notice-link",
              onClick: () => {
                options.action?.onClick();
                notifications.clean();
              },
            },
            options.action.label,
          ),
        )
      : message,
    color: options?.error ? "red" : "cyan",
    autoClose: options?.error ? 7000 : 4500,
  });
}

function managedModelPayload(models: ManagedPresetModel[]) {
  return models.map((model) =>
    modelSelectionPayload({
      sourceKind: model.sourceKind,
      sourceId: model.sourceId,
      upstreamModelId: model.upstreamModelId,
      publicModelId: model.publicModelId,
      displayName: model.displayName,
      contextWindow: optionalWindowValue(model.contextWindow),
      autoCompactTokenLimit: optionalWindowValue(model.autoCompactTokenLimit),
      reasoningLevels: model.reasoningLevels,
      defaultReasoningLevel: model.defaultReasoningLevel ?? undefined,
    }),
  );
}

/**
 * @function useAppController
 * @description 管理 R Codex 控制面快照、弹层状态和用户触发的 IPC 操作。
 */
export function useAppController() {
  const [utilityDrawer, setUtilityDrawer] = useState<UtilityDrawerView | null>(null);
  const [scopedPresetId, setScopedPresetId] = useState<string | null>(null);
  const [themeMode, setThemeMode] = useState<ThemeMode>(getInitialThemeMode);
  const [accent, setAccent] = useState(getInitialAccent);
  const [systemDark, setSystemDark] = useState(prefersDarkTheme);
  const [dismissedIncidentAt, setDismissedIncidentAt] = useState<number | null>(null);
  const [gateway, setGateway] = useState<GatewayStatus>(stoppedGateway);
  const [providers, setProviders] = useState<Provider[]>([]);
  const [accounts, setAccounts] = useState<OfficialAccountSummary[]>([]);
  const [presets, setPresets] = useState<LaunchPresetSummary[]>([]);
  const [instances, setInstances] = useState<RuntimeInstanceSummary[]>([]);
  const [compatibility, setCompatibility] = useState<CodexCompatibility>({
    controlPlaneAvailable: false,
    officialAccountsAvailable: false,
    managedAppAvailable: false,
    officialHybridAvailable: false,
  });
  const [codexInstallation, setCodexInstallation] = useState<CodexInstallationStatus | null>(null);
  const [codexUpdate, setCodexUpdate] = useState<CodexUpdateStatus | null>(null);
  const [appUpdate, setAppUpdate] = useState<AppReleaseUpdate | null>(null);
  const [appUpdateBusy, setAppUpdateBusy] = useState(false);
  const appUpdateCheckedRef = useRef(false);
  const installAppUpdateRef = useRef<() => Promise<void>>(async () => {});
  const [usageByProvider, setUsageByProvider] = useState<Record<string, ProviderUsageSummary>>({});
  const [connectivityByProvider, setConnectivityByProvider] = useState<Record<string, ProviderConnectivityReport>>({});
  const [specialModelRoles, setSpecialModelRoles] = useState<SpecialModelRoleSummary[]>([]);
  const [diagnostics, setDiagnostics] = useState<GatewayDiagnosticEvent[]>([]);
  const [requestDiagnostics, setRequestDiagnostics] = useState<InstanceRequestDiagnostic[]>([]);
  const [notice, setNotice] = useState("");
  const [booting, setBooting] = useState(true);
  const [noticeError, setNoticeError] = useState<CommandError | null>(null);
  const [pendingKeys, setPendingKeys] = useState<Set<string>>(() => new Set());
  const [isRefreshing, setIsRefreshing] = useState(false);
  const [testingProviderId, setTestingProviderId] = useState<string | null>(null);
  const [showAddProvider, setShowAddProvider] = useState(false);
  const [selectedProviderId, setSelectedProviderId] = useState<string | null>(null);
  const [selectedProviderModelCatalogId, setSelectedProviderModelCatalogId] = useState<string | null>(null);
  const [deleteCandidate, setDeleteCandidate] = useState<Provider | null>(null);
  const [showClearDiagnostics, setShowClearDiagnostics] = useState(false);
  const [showClearInstances, setShowClearInstances] = useState(false);
  const [launchCandidate, setLaunchCandidate] = useState<LaunchPresetSummary | null>(null);
  const [launchPreferredTarget, setLaunchPreferredTarget] = useState<LaunchTarget>("cli");
  const [focusedInstanceIds, setFocusedInstanceIds] = useState<string[]>([]);
  const [selectedPresetDetails, setSelectedPresetDetails] = useState<LaunchPresetSummary | null>(null);
  const [instanceDeleteCandidate, setInstanceDeleteCandidate] = useState<RuntimeInstanceSummary | null>(null);
  const [instanceClearCandidate, setInstanceClearCandidate] = useState<RuntimeInstanceSummary | null>(null);
  const [instanceDiagnosticsById, setInstanceDiagnosticsById] = useState<Record<string, InstanceRequestDiagnostic[]>>({});
  const [instanceSummariesById, setInstanceSummariesById] = useState<Record<string, ConversationSummary[]>>({});
  const [instanceDiagnosticsLoading, setInstanceDiagnosticsLoading] = useState<string | null>(null);
  const refreshPromiseRef = useRef<Promise<void> | null>(null);
  const bootStartedRef = useRef(Date.now());
  const bootOpenRef = useRef(true);
  const bootTimerRef = useRef<number | null>(null);
  const gatewayUnavailableNotedRef = useRef(false);

  const selectedProvider = useMemo(() => providers.find((item) => item.id === selectedProviderId), [providers, selectedProviderId]);
  const selectedProviderModelCatalog = useMemo(
    () => providers.find((item) => item.id === selectedProviderModelCatalogId),
    [providers, selectedProviderModelCatalogId],
  );
  const activeInstanceIds = useMemo(
    () => instances.filter((item) => isActiveInstanceState(item.state)).map((item) => item.id).sort().join(","),
    [instances],
  );
  const uiBusy = isRefreshing;
  const noticeIsError = noticeError !== null;
  const resolvedTheme = themeMode === "system" ? (systemDark ? "dark" : "light") : themeMode;
  const pendingKey = (key: string) => pendingKeys.has(key);
  const beginPending = (key: string) => setPendingKeys((current) => new Set(current).add(key));
  const endPending = (key: string) => setPendingKeys((current) => {
    const next = new Set(current);
    next.delete(key);
    return next;
  });
  const showInfo = (message: string) => {
    setNoticeError(null);
    setNotice(message);
    showAppNotice(message);
  };
  const showCommandError = (error: unknown, prefix: string) => {
    const parsed = toCommandError(error);
    const message = `${prefix}：${parsed.message}`;
    setNoticeError(parsed);
    setNotice(message);
    showAppNotice(message, { error: true });
    if (parsed.code === "configuration_in_use") {
      setFocusedInstanceIds(parsed.conflictingInstances.map((item) => item.instanceId));
    }
  };

  async function refresh(options: { probeInstallation?: boolean } = {}) {
    if (refreshPromiseRef.current) return refreshPromiseRef.current;
    const shouldProbeInstallation = options.probeInstallation === true || codexInstallation === null;
    const request = (async () => {
      const installationRequest = shouldProbeInstallation
        ? invoke<CodexInstallationStatus>("codex_installation_status")
        : Promise.resolve(codexInstallation);
      const [status, snapshot, installation] = await Promise.all([
        invoke<GatewayStatus>("gateway_status"),
        getCodexControlSnapshot(),
        installationRequest,
      ]);
      setGateway(status);
      if (status.state === "unavailable") {
        if (!gatewayUnavailableNotedRef.current) {
          gatewayUnavailableNotedRef.current = true;
          showAppNotice("本机 Gateway 失联。标题栏可以重启它。正在进行的请求会中断，活动绑定会从数据库恢复。", { error: true });
        }
      } else {
        gatewayUnavailableNotedRef.current = false;
      }
      setProviders(snapshot.providers);
      setAccounts(snapshot.accounts);
      setPresets(snapshot.presets);
      setInstances(snapshot.instances);
      setCompatibility(snapshot.compatibility);
      if (shouldProbeInstallation && installation) setCodexInstallation(installation);
      if (shouldProbeInstallation && installation?.installed) {
        setCodexUpdate(
          await invoke<CodexUpdateStatus>("check_codex_updates").catch(() => ({
            state: "unavailable",
            checkedAt: Math.floor(Date.now() / 1000),
            error: "版本检查命令不可用",
          })),
        );
      }
      const [roles, summaries, reports, events, requests] = await Promise.all([
        getCodexSpecialModelRoles().catch(() => []),
        Promise.all(snapshot.providers.map(async (provider) => {
          try {
            return [provider.id, await invoke<ProviderUsageSummary>("get_provider_usage_summary", { providerId: provider.id })] as const;
          } catch {
            return null;
          }
        })),
        Promise.all(snapshot.providers.map((provider) =>
          invoke<ProviderConnectivityReport | null>("get_provider_connectivity_report", { providerId: provider.id }).catch(() => null),
        )),
        invoke<GatewayDiagnosticEvent[]>("gateway_recent_diagnostics").catch(() => []),
        invoke<InstanceRequestDiagnostic[]>("recent_gateway_requests").catch(() => []),
      ]);
      setSpecialModelRoles(roles);
      setUsageByProvider(Object.fromEntries(summaries.filter((entry): entry is readonly [string, ProviderUsageSummary] => entry !== null)));
      setConnectivityByProvider(Object.fromEntries(reports.filter((entry): entry is ProviderConnectivityReport => entry !== null).map((entry) => [entry.providerId, entry])));
      setDiagnostics(events);
      setRequestDiagnostics(requests);
    })();
    refreshPromiseRef.current = request;
    setIsRefreshing(true);
    try {
      await request;
    } finally {
      if (refreshPromiseRef.current === request) refreshPromiseRef.current = null;
      setIsRefreshing(false);
      if (bootOpenRef.current) {
        bootOpenRef.current = false;
        bootTimerRef.current = window.setTimeout(
          () => setBooting(false),
          Math.max(0, 720 - (Date.now() - bootStartedRef.current)),
        );
      }
    }
  }


  const installAppUpdate = async () => {
    if (!appUpdate) return;
    const active = instances.some((instance) => isActiveInstanceState(instance.state));
    if (active) {
      showAppNotice("隔离实例正在运行，请先停止后再安装更新。", { error: true });
      return;
    }
    setAppUpdateBusy(true);
    try {
      showInfo("正在下载并安装 R Codex 更新…");
      await appUpdate.install();
    } catch (error: unknown) {
      setAppUpdateBusy(false);
      showCommandError(error, "安装更新失败");
    }
  };

  const openReleasePage = () => {
    void openAppReleasePage().catch((error: unknown) => showCommandError(error, "无法打开发布页"));
  };
  installAppUpdateRef.current = installAppUpdate;

  useEffect(() => {
    void refresh().catch((error: unknown) => showCommandError(error, "无法读取应用状态"));
  }, []);

  useEffect(() => {
    if (booting || appUpdateCheckedRef.current) return;
    appUpdateCheckedRef.current = true;
    void checkAppRelease()
      .then((update) => {
        if (!update) return;
        setAppUpdate(update);
        showAppNotice("R Codex " + update.version + " 可用", {
          action: { label: "安装并重启", onClick: () => { void installAppUpdateRef.current(); } },
        });
      })
      .catch(() => undefined);
  }, [booting]);
  useEffect(() => () => {
    if (bootTimerRef.current !== null) window.clearTimeout(bootTimerRef.current);
  }, []);
  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void listen("codex-control-snapshot-invalidated", () => {
      void refresh().catch((error: unknown) => showCommandError(error, "无法刷新运行状态"));
    }).then((dispose) => {
      if (disposed) dispose();
      else unlisten = dispose;
    }).catch(() => undefined);
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);
  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void listen<{ error?: string }>("app-exit-blocked", (event) => {
      const error = event.payload?.error;
      showAppNotice(
        error ? `退出被拦住：${error}` : "退出被拦住：隔离实例或 Gateway 没有停干净。",
        { error: true },
      );
    }).then((dispose) => {
      if (disposed) dispose();
      else unlisten = dispose;
    }).catch(() => undefined);
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);
  useEffect(() => {
    if (!activeInstanceIds) return undefined;
    let disposed = false;
    let timer: number | undefined;
    const poll = async () => {
      try {
        const reconciled = await Promise.all(activeInstanceIds.split(",").map((instanceId) =>
          invokeControl<RuntimeInstanceSummary>("reconcile_codex_instance", { instanceId }),
        ));
        if (!disposed) {
          const changed = new Map(reconciled.map((instance) => [instance.id, instance]));
          setInstances((current) => current.map((instance) => changed.get(instance.id) ?? instance));
          if (reconciled.some((instance) => !isActiveInstanceState(instance.state))) {
            void refresh();
          }
        }
      } catch {
        // The next poll retries a transient IPC failure.
      }
      if (!disposed) timer = window.setTimeout(() => void poll(), 2000);
    };
    void poll();
    return () => {
      disposed = true;
      if (timer !== undefined) window.clearTimeout(timer);
    };
  }, [activeInstanceIds]);
  useEffect(() => {
    document.documentElement.dataset.chrome = desktopChrome();
    document.documentElement.style.colorScheme = resolvedTheme;
    try {
      window.localStorage.setItem(themeStorageKey, themeMode);
      window.localStorage.setItem(accentStorageKey, accent);
      window.localStorage.removeItem("rcodex-component-size");
    } catch { /* unavailable */ }
  }, [accent, resolvedTheme, themeMode]);
  useEffect(() => {
    if (themeMode !== "system") return undefined;
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    const update = () => setSystemDark(media.matches);
    update();
    media.addEventListener("change", update);
    return () => media.removeEventListener("change", update);
  }, [themeMode]);
  useEffect(() => {
    if (selectedProviderId && !selectedProvider) setSelectedProviderId(null);
  }, [selectedProvider, selectedProviderId]);
  useEffect(() => {
    if (selectedProviderModelCatalogId && !selectedProviderModelCatalog) setSelectedProviderModelCatalogId(null);
  }, [selectedProviderModelCatalog, selectedProviderModelCatalogId]);

  async function stopGateway() {
    beginPending("gateway:stop");
    try {
      setGateway(await invoke<GatewayStatus>("stop_gateway"));
      showInfo("Gateway 已停止。");
    } catch (error) {
      showCommandError(error, "停止 Gateway 失败");
    } finally {
      endPending("gateway:stop");
    }
  }
  async function restartLostGateway() {
    beginPending("gateway:restart");
    try {
      setGateway(await invoke<GatewayStatus>("restart_unavailable_gateway"));
      showInfo("已关闭失联的旧 Gateway，并启动了新的 sidecar。");
    } catch (error) {
      showCommandError(error, "重启 Gateway 失败");
    } finally {
      endPending("gateway:restart");
    }
  }
  async function createProvider(draft: ProviderChannelDraft) {
    const key = "provider:create";
    beginPending(key);
    try {
      const saved = await invoke<Provider & { probeAttachment?: string }>("create_api_key_provider", {
        input: {
          name: draft.name,
          baseUrl: draft.baseUrl,
          apiKey: draft.apiKey,
          transportMode: "gateway_passthrough",
          gatewayTimeoutSettings: draft.gatewaySettings,
          ...(draft.probeTicketId ? { probeTicketId: draft.probeTicketId } : {}),
        },
      });
      setShowAddProvider(false);
      await refresh();
      showInfo(saved.probeAttachment === "attached"
        ? `模型渠道“${saved.name}”已保存，探针结果已写入。`
        : `模型渠道“${saved.name}”与 API Key 已安全保存；请在工作区选择模型。`);
    } catch (error) {
      showCommandError(error, "无法保存模型渠道");
    } finally {
      endPending(key);
    }
  }
  async function probeChannelDraft(baseUrl: string, apiKey: string) {
    try {
      return await invoke<DraftChannelProbe>("probe_channel_draft", { baseUrl, apiKey });
    } catch (error) {
      showCommandError(error, "渠道探针失败");
      throw error;
    }
  }
  function requestDeleteProvider(provider: Provider) { setDeleteCandidate(provider); }
  async function confirmDeleteProvider() {
    if (!deleteCandidate) return;
    const providerId = deleteCandidate.id;
    const key = `provider:${providerId}`;
    beginPending(key);
    try {
      await invoke("delete_api_key_provider", { providerId });
      setDeleteCandidate(null);
      if (selectedProviderId === providerId) setSelectedProviderId(null);
      if (selectedProviderModelCatalogId === providerId) setSelectedProviderModelCatalogId(null);
      await refresh();
      showInfo("模型渠道、系统凭据和 Gateway 设置已移除。");
    } catch (error) {
      showCommandError(error, "无法删除模型渠道");
    } finally {
      endPending(key);
    }
  }
  async function renameProvider(providerId: string, name: string) {
    const key = `provider:${providerId}`;
    beginPending(key);
    try {
      const saved = await invoke<Provider>("rename_api_key_provider", { providerId, name });
      await refresh();
      showInfo(`模型渠道已改名为“${saved.name}”。正在运行的实例仍使用启动时的名称，下次启动才换上新名称。`);
      return true;
    } catch (error) {
      showCommandError(error, "无法修改渠道名称");
      return false;
    } finally {
      endPending(key);
    }
  }
  async function clearDiagnostics() {
    const key = "diagnostics:clear";
    beginPending(key);
    try {
      await invoke("clear_gateway_diagnostics");
      setShowClearDiagnostics(false);
      setDiagnostics([]);
      setRequestDiagnostics([]);
      showInfo("已清空本地诊断记录。");
    } catch (error) {
      showCommandError(error, "无法清空诊断记录");
    } finally {
      endPending(key);
    }
  }
  async function loadInstanceDiagnostics(instanceId: string) {
    setInstanceDiagnosticsLoading(instanceId);
    try {
      const [events, summaries] = await Promise.all([
        invoke<InstanceRequestDiagnostic[]>("instance_request_diagnostics", { instanceId }),
        invoke<ConversationSummary[]>("instance_conversation_summaries", { instanceId }),
      ]);
      setInstanceDiagnosticsById((current) => ({ ...current, [instanceId]: events }));
      setInstanceSummariesById((current) => ({ ...current, [instanceId]: summaries }));
    } catch (error) {
      showCommandError(error, "无法读取实例诊断");
    } finally {
      setInstanceDiagnosticsLoading(null);
    }
  }
  async function clearInstanceDiagnostics(instance: RuntimeInstanceSummary) {
    const key = `instance:${instance.id}`;
    beginPending(key);
    try {
      await invoke("clear_instance_diagnostics", { instanceId: instance.id });
      setInstanceClearCandidate(null);
      setInstanceDiagnosticsById((current) => ({ ...current, [instance.id]: [] }));
      setInstanceSummariesById((current) => ({ ...current, [instance.id]: [] }));
      showInfo("已清除该实例的诊断记录。");
    } catch (error) {
      showCommandError(error, "无法清除实例诊断");
    } finally {
      endPending(key);
    }
  }
  function workspaceInput(preset: LaunchPresetSummary, changes: Partial<{ providerIds: string[]; models: ManagedPresetModel[]; createNew: boolean; name: string }> = {}) {
    return {
      name: changes.name ?? preset.name,
      entryKind: "managed_app",
      officialAccountId: preset.officialAccountId ?? null,
      apiKeyProviderIds: changes.providerIds ?? preset.providerIds,
      transportMode: "gateway_passthrough",
      models: managedModelPayload(changes.models ?? preset.models),
      autoReviewPublicModelId: preset.autoReviewPublicModelId ?? null,
      defaultWorkspacePath: null,
      ...(changes.createNew ? { createNew: true } : { launchPresetId: preset.id }),
    };
  }
  async function saveWorkspace(preset: LaunchPresetSummary, changes: Parameters<typeof workspaceInput>[1], success: string) {
    const key = `preset:${preset.id}`;
    beginPending(key);
    try {
      await invokeControl("create_managed_launch_preset", { input: workspaceInput(preset, changes) });
      await refresh();
      showInfo(runningWorkspaceRestartNotice(instances, preset.id) ?? success);
    } catch (error) {
      showCommandError(error, "无法更新隔离工作区");
    } finally {
      endPending(key);
    }
  }
  function removeProviderFromWorkspace(preset: LaunchPresetSummary, providerId: string) {
    void saveWorkspace(preset, {
      providerIds: preset.providerIds.filter((id) => id !== providerId),
      models: preset.models.filter((model) => model.sourceId !== providerId),
    }, "模型渠道已移出隔离工作区。");
  }
  function addProviderToWorkspace(preset: LaunchPresetSummary, providerId: string) {
    if (!preset.providerIds.includes(providerId)) {
      void saveWorkspace(preset, { providerIds: [providerId, ...preset.providerIds] }, "模型渠道已加入隔离工作区。");
    }
  }
  function reorderProvidersInWorkspace(preset: LaunchPresetSummary, providerIds: string[]) {
    if (providerIds.length === preset.providerIds.length) {
      void saveWorkspace(preset, { providerIds }, "模型渠道顺序已保存。");
    }
  }
  function toggleWorkspaceModelSelection(preset: LaunchPresetSummary, providerId: string, upstreamModelId: string) {
    const current = preset.models.find((item) => item.sourceId === providerId && item.upstreamModelId === upstreamModelId);
    if (isImageGenerationModelId(upstreamModelId)) {
      if (!current) {
        return showInfo("这个模型用于图像生成，不会显示在工作区勾选中，也不能作为对话模型加入工作区。");
      }
      void saveWorkspace(preset, {
        models: preset.models.filter((item) => item !== current),
      }, "已从工作区移除图像生成模型。");
      return;
    }
    if (isInternalCodexModelId(upstreamModelId)) {
      return showInfo("这个模型是 Codex 系统角色，用于自动审核或子代理等特殊调用，不能作为普通模型加入工作区。");
    }
    const provider = providers.find((item) => item.id === providerId);
    const option = buildDiscoveredModelOptions([{ providerId, providerName: provider?.name ?? "模型渠道", models: selectableProviderModels(connectivityByProvider[providerId]?.models ?? []) }])
      .find((item) => item.upstreamModelId === upstreamModelId);
    if (!option) return showInfo("请先刷新模型目录，再选择该模型。");
    const models = current
      ? preset.models.filter((item) => item !== current)
      : [...preset.models, {
          routeIdentityId: "",
          sourceKind: "api_provider" as const,
          sourceId: providerId,
          upstreamModelId,
          publicModelId: option.publicModelId,
          displayName: option.displayLabel,
        }];
    void saveWorkspace(preset, {
      models: models.filter((item) => isWorkspaceSelectableModelId(item.upstreamModelId) && isWorkspaceSelectableModelId(item.publicModelId)),
    }, "工作区模型已更新。");
  }
  function setDefaultWorkspaceModel(preset: LaunchPresetSummary, modelId: string) {
    const selected = preset.models.find((item) => item.publicModelId === modelId);
    if (selected) void saveWorkspace(preset, { models: [selected, ...preset.models.filter((item) => item !== selected)] }, "默认调用模型已更新。");
  }
  async function setManagedLoginProvider(preset: LaunchPresetSummary, providerId: string) {
    const key = `preset:${preset.id}`;
    beginPending(key);
    try {
      await invokeControl("set_managed_login_provider", { input: { launchPresetId: preset.id, expectedVersion: preset.version, providerId: providerId || null } });
      await refresh();
      showInfo(runningWorkspaceRestartNotice(instances, preset.id) ?? "API 登录渠道已更新。");
    } catch (error) {
      showCommandError(error, "无法更新 API 登录渠道");
    } finally {
      endPending(key);
    }
  }
  async function selectManagedWorkspace(preset: LaunchPresetSummary) {
    setScopedPresetId(preset.id);
    try {
      await invokeControl("select_managed_launch_preset", { input: { launchPresetId: preset.id } });
      await refresh();
    } catch (error) {
      showCommandError(error, "无法切换隔离实例");
    }
  }
  async function createManagedInstance(source?: LaunchPresetSummary) {
    const preset = source ?? presets.find((item) => item.entryKind === "managed_app");
    if (!preset) return null;
    const key = "preset:create";
    beginPending(key);
    try {
      const saved = await invokeControl<{ id: string }>("create_managed_launch_preset", { input: workspaceInput(preset, { name: "R Codex", createNew: true }) });
      await refresh();
      showInfo("已新增隔离实例。会话目录和 Electron 桌面数据都独立，可以和其他实例同时运行。");
      return saved.id;
    } catch (error) {
      showCommandError(error, "无法新增实例");
      return null;
    } finally {
      endPending(key);
    }
  }
  async function renameManagedInstance(preset: LaunchPresetSummary, name: string) {
    const key = `preset:${preset.id}`;
    beginPending(key);
    try {
      await invokeControl("rename_launch_preset", { launchPresetId: preset.id, input: { expectedVersion: preset.version, name } });
      await refresh();
      showInfo("实例名称已更新。");
    } catch (error) {
      showCommandError(error, "无法更新实例名称");
    } finally {
      endPending(key);
    }
  }
  async function openDefaultCodex() {
    const key = "default-codex";
    beginPending(key);
    try {
      await invoke("open_codex_app");
      showInfo("已请求打开默认 Codex。");
    } catch (error) {
      showCommandError(error, "无法打开默认 Codex");
    } finally {
      endPending(key);
    }
  }
  async function openIsolatedCodexHome(preset: LaunchPresetSummary) {
    const key = "codex-home";
    beginPending(key);
    try {
      await invoke("open_isolated_codex_home", { launchPresetId: preset.id });
      showInfo("已打开隔离 CODEX_HOME。");
    } catch (error) {
      showCommandError(error, "无法打开隔离目录");
    } finally {
      endPending(key);
    }
  }
  function requestLaunchPreset(preset: LaunchPresetSummary, target: LaunchTarget = "cli") {
    if (!OFFICIAL_AUTH_ENABLED && usesOfficialModel(preset)) {
      showInfo("官方账户认证暂时关闭；请选择 API Key 模型渠道。");
      return;
    }
    if (isManagedAppTarget(target)) {
      void launchPreset(preset, "", target);
      return;
    }
    setLaunchPreferredTarget(target);
    setLaunchCandidate(preset);
  }
  async function refreshIfLaunchUnconfirmed(error: unknown) {
    if (toCommandError(error).code !== "launch_unconfirmed") return;
    try {
      await refresh();
    } catch {
      // The launch notice still explains that the receipt has not arrived.
    }
  }
  async function launchPreset(preset: LaunchPresetSummary, workingDirectory: string, target: LaunchTarget = "cli") {
    const key = `preset:${preset.id}`;
    beginPending(key);
    try {
      const workspacePath = isManagedAppTarget(target) ? undefined : workingDirectory.trim() || undefined;
      await invokeControl<RuntimeInstanceSummary>("launch_codex", {
        input: {
          launchPresetId: preset.id,
          expectedVersion: preset.version,
          launchTarget: target,
          ...(workspacePath ? { workspacePath } : {}),
        },
      });
      setLaunchCandidate(null);
      await refresh();
      showInfo(`已通过“${preset.name}”启动${isManagedAppTarget(target) ? "隔离 Codex App" : "隔离 Codex CLI"}。`);
    } catch (error) {
      await refreshIfLaunchUnconfirmed(error);
      showCommandError(error, "无法启动 Codex");
    } finally {
      endPending(key);
    }
  }
  async function testProvider(providerId: string) {
    const key = `provider:${providerId}`;
    beginPending(key);
    setTestingProviderId(providerId);
    try {
      const report = await invoke<ProviderConnectivityReport>("test_api_key_provider", { providerId });
      setConnectivityByProvider((current) => ({ ...current, [providerId]: report }));
      let balance: UpstreamBalance | null = null;
      try {
        balance = await invoke<UpstreamBalance>("refresh_provider_upstream_balance", { providerId });
      } catch {
        // Balance APIs are optional for Provider connectivity.
      }
      const summary = await invoke<ProviderUsageSummary>("get_provider_usage_summary", { providerId });
      setUsageByProvider((current) => ({ ...current, [providerId]: summary }));
      setDiagnostics(await invoke<GatewayDiagnosticEvent[]>("gateway_recent_diagnostics"));
      showInfo(report.status === "ready"
        ? balance?.status === "supported"
          ? "模型渠道连通性通过，余额已更新。"
          : "模型渠道连通性通过。"
        : "模型渠道检测完成，请查看状态详情。");
    } catch (error) {
      showCommandError(error, "模型渠道检测失败");
    } finally {
      setTestingProviderId(null);
      endPending(key);
    }
  }
  async function stopInstance(instance: RuntimeInstanceSummary) {
    const key = `instance:${instance.id}`;
    beginPending(key);
    try {
      await invokeControl("stop_codex_instance", { instanceId: instance.id });
      await refresh();
      showInfo("已请求停止所选 R Codex 实例。");
    } catch (error) {
      showCommandError(error, "无法停止 Codex 实例");
    } finally {
      endPending(key);
    }
  }
  async function restartInstance(instance: RuntimeInstanceSummary) {
    const preset = presets.find((item) => item.id === instance.launchPresetId);
    if (!preset) return showInfo("找不到该实例的启动方案，无法安全重启。");
    const key = `instance:${instance.id}`;
    beginPending(key);
    try {
      await invokeControl("stop_codex_instance", { instanceId: instance.id });
      await invokeControl<RuntimeInstanceSummary>("launch_codex", {
        input: {
          launchPresetId: preset.id,
          expectedVersion: preset.version,
          launchTarget: instance.launchTarget ?? "cli",
          ...(instance.workspacePath && !isManagedAppTarget(instance.launchTarget)
            ? { workspacePath: instance.workspacePath }
            : {}),
        },
      });
      await refresh();
      showInfo(`已重新启动“${preset.name}”。`);
    } catch (error) {
      await refreshIfLaunchUnconfirmed(error);
      showCommandError(error, "无法重启 Codex 实例");
    } finally {
      endPending(key);
    }
  }
  async function reconcileInstance(instance: RuntimeInstanceSummary) {
    const key = `instance:${instance.id}`;
    beginPending(key);
    try {
      await invokeControl("reconcile_codex_instance", { instanceId: instance.id });
      await refresh();
      showInfo("实例进程状态已重新核验。");
    } catch (error) {
      showCommandError(error, "无法核验 Codex 实例");
    } finally {
      endPending(key);
    }
  }
  async function deleteInstance(instance: RuntimeInstanceSummary) {
    const key = `instance:${instance.id}`;
    beginPending(key);
    try {
      await invokeControl("delete_codex_instance", { instanceId: instance.id });
      setInstanceDeleteCandidate(null);
      setFocusedInstanceIds((current) => current.filter((id) => id !== instance.id));
      await refresh();
      showInfo("实例及其隔离数据已删除。");
    } catch (error) {
      showCommandError(error, "无法删除 Codex 实例记录");
    } finally {
      endPending(key);
    }
  }
  async function clearSafeInstances() {
    const key = "instances:clear";
    beginPending(key);
    try {
      const result = await invoke<{ deleted: number }>("delete_safe_codex_instances");
      setShowClearInstances(false);
      await refresh();
      showInfo(`已清除 ${result.deleted} 条可安全删除的实例记录。`);
    } catch (error) {
      showCommandError(error, "无法清除可安全删除的实例");
    } finally {
      endPending(key);
    }
  }
  function viewInstances(ids: string[]) {
    setFocusedInstanceIds(ids);
    setUtilityDrawer("instances");
    window.setTimeout(() => {
      document.getElementById(`instance-${ids[0]}`)?.scrollIntoView({ block: "center" });
    }, 0);
  }
  function preferredPresetId() {
    return (
      presets.find((preset) => preset.entryKind === "managed_app" && preset.isPreferred)?.id ??
      presets.find((preset) => preset.entryKind === "managed_app")?.id ??
      null
    );
  }
  function openUtility(view: UtilityDrawerView, presetId?: string | null) {
    const next = presetId ?? scopedPresetId ?? preferredPresetId();
    if (next) setScopedPresetId(next);
    setUtilityDrawer(view);
  }
  function openAddProvider() {
    setSelectedProviderId(null);
    setUtilityDrawer("credentials");
    setShowAddProvider(true);
  }

  const gatewayStopReason = gateway.activeBindings > 0 ? "Gateway 正被活动路由使用。" : undefined;
  const gatewayRestartBusy = pendingKeys.has("gateway:restart");
  const providerNames = Object.fromEntries(providers.map((provider) => [provider.id, provider.name]));
  const latestFailure = [
    ...diagnostics.filter((event) => event.outcome === "failed").map((event) => ({
      at: event.createdAt,
      title: `${providerNames[event.providerId] ?? "模型渠道"} 检测失败`,
      detail: event.errorCode ?? "查看诊断",
    })),
    ...requestDiagnostics.filter((event) => event.outcome !== "success").map((event) => ({
      at: event.createdAt,
      title: `${event.publicModelId || providerNames[event.providerId] || "请求"} 失败`,
      detail: event.errorCode ?? "查看诊断",
    })),
  ].sort((left, right) => right.at - left.at)[0] ?? null;
  const diagnosticCount = diagnostics.length + requestDiagnostics.length;
  const gatewayEndpoint = gateway.port === null ? null : `${gateway.bindHost}:${gateway.port}`;
  const activeManagedAppInstanceCount = instances.filter((instance) =>
    isActiveInstanceState(instance.state) && isManagedAppTarget(instance.launchTarget),
  ).length;
  const visibleManagedInstances = instances.filter((instance) => isKnownLaunchTarget(instance.launchTarget));
  const diagnosticModels: DiagnosticEventModel[] = diagnostics.map((event) => ({
    id: event.id, providerId: event.providerId, createdAt: event.createdAt,
    source: event.source, route: event.route, model: event.model, stream: event.stream,
    outcome: event.outcome, httpStatus: event.httpStatus, latencyMs: event.latencyMs,
    errorCode: event.errorCode,
  }));
  const requestModels: RequestEventModel[] = requestDiagnostics.map((event) => ({
    id: event.id, providerId: event.providerId, publicModelId: event.publicModelId,
    upstreamModelId: event.upstreamModelId, createdAt: event.createdAt,
    instanceId: event.instanceId, wireApi: event.wireApi, stream: event.stream,
    outcome: event.outcome, httpStatus: event.httpStatus, errorCode: event.errorCode,
    errorDetail: event.errorDetail, totalLatencyMs: event.totalLatencyMs,
  }));

  return {
    utilityDrawer, setUtilityDrawer, scopedPresetId, openUtility, themeMode, setThemeMode, accent, setAccent, systemDark, setSystemDark,
    dismissedIncidentAt, setDismissedIncidentAt, gateway, setGateway, providers, setProviders,
    accounts, setAccounts, presets, setPresets, instances, setInstances, compatibility,
    setCompatibility, codexInstallation, setCodexInstallation, codexUpdate, setCodexUpdate, appUpdate, appUpdateBusy, installAppUpdate, openReleasePage,
    usageByProvider, setUsageByProvider, connectivityByProvider, setConnectivityByProvider,
    specialModelRoles, setSpecialModelRoles, diagnostics, setDiagnostics, requestDiagnostics,
    setRequestDiagnostics, notice, setNotice, booting, setBooting, noticeError, setNoticeError,
    pendingKeys, setPendingKeys, isRefreshing, setIsRefreshing, testingProviderId,
    setTestingProviderId, showAddProvider, setShowAddProvider, selectedProviderId,
    setSelectedProviderId, selectedProviderModelCatalogId, setSelectedProviderModelCatalogId,
    deleteCandidate, setDeleteCandidate, showClearDiagnostics, setShowClearDiagnostics,
    showClearInstances, setShowClearInstances, launchCandidate, setLaunchCandidate,
    launchPreferredTarget, setLaunchPreferredTarget, focusedInstanceIds, setFocusedInstanceIds,
    selectedPresetDetails, setSelectedPresetDetails, instanceDeleteCandidate,
    setInstanceDeleteCandidate, instanceClearCandidate, setInstanceClearCandidate,
    instanceDiagnosticsById, setInstanceDiagnosticsById, instanceSummariesById,
    setInstanceSummariesById, instanceDiagnosticsLoading, setInstanceDiagnosticsLoading,
    selectedProvider, selectedProviderModelCatalog, activeInstanceIds, uiBusy, noticeIsError,
    resolvedTheme, pendingKey, beginPending, endPending, showInfo, showCommandError, refresh,
    stopGateway, restartLostGateway, gatewayRestartBusy, createProvider, probeChannelDraft, requestDeleteProvider, confirmDeleteProvider,
    renameProvider,
    clearDiagnostics, loadInstanceDiagnostics, clearInstanceDiagnostics, openDefaultCodex,
    openIsolatedCodexHome, requestLaunchPreset, launchPreset, testProvider,
    removeProviderFromWorkspace, addProviderToWorkspace, reorderProvidersInWorkspace,
    toggleWorkspaceModelSelection, setDefaultWorkspaceModel, setManagedLoginProvider,
    selectManagedWorkspace, createManagedInstance, renameManagedInstance, stopInstance,
    restartInstance, reconcileInstance, deleteInstance, clearSafeInstances, viewInstances,
    openAddProvider, gatewayStopReason, providerNames, latestFailure,
    diagnosticCount, gatewayEndpoint, activeManagedAppInstanceCount, visibleManagedInstances,
    diagnosticModels, requestModels,
  };
}
