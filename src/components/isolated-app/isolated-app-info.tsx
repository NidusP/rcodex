import { useState } from "react";
import { AppWindow, FolderOpen, Info, ScrollText } from "lucide-react";
import {
  isActiveInstanceState,
  isManagedAppTarget,
  type ApiKeyProviderSummary,
  type LaunchPresetSummary,
  type OfficialAccountSummary,
  type RuntimeInstanceSummary,
} from "../../codex-control";
import {
  ChannelWorkspaceCard,
  usageObservationLabel,
  type WorkspaceChannelChoice,
  type WorkspaceChannelRow,
  type WorkspaceInstanceTab,
} from "../../workspace-surface";
import { HintedButton } from "../hinted-button";
import { useAppState } from "../app-state";
import { isModelExposed } from "../../model-exposure";
import { compareWorkspaceOrder } from "../../workspace-order";
import { WorkspaceDeleteDialog } from "../workspace-delete";
import {
  buildDiscoveredModelOptions,
} from "../../workspace-model-selection";
import {
  compareModelSortText,
  connectivityLabels,
  hiddenCodexRoleModelCount,
  isImageGenerationModelId,
  isWorkspaceSelectableModelId,
  protocolText,
  providerHost,
  qualifiedModelLabel,
  selectableProviderModels,
  sortedManagedModels,
  visibleProviderModels,
  type ConnectivityLabelStatus,
} from "../model-labels";

import "./isolated-app-info.css";

type UsageView = {
  today: { requests: number };
  lastRequestAt: number | null;
};
type ConnectivityView = {
  status: ConnectivityLabelStatus;
  wireApi: "responses" | "chat_completions" | null;
  models: Array<{
    upstreamModelId: string;
    displayName: string;
    ownedBy: string | null;
  }>;
};
type UtilityDrawerView = "credentials" | "instances" | "diagnostics";

/**
 * @component IsolatedAppBody
 * @description 独立工作区主体：实例标签、渠道列表和启动控制带。
 */
function IsolatedAppBody({
  presets,
  providers,
  accounts,
  pendingKey,
  usageByProvider,
  connectivityByProvider,
  testingProviderId,
  onLaunchApp,
  onLaunchCli,
  onToggleModel,
  onAddProvider,
  onAddProviderToNewWorkspace,
  onRemoveProvider,
  onDefaultModelChange,
  onLoginProviderChange,
  onOpenUtility,
  onOpenAddProvider,
  onOpenDefaultCodex,
  onOpenCodexHome,
  onTestProvider,
  onReorderChannels,
  onSelectInstance,
  onCreateInstance,
  instances,
  onStopInstance,
}: {
  presets: LaunchPresetSummary[];
  providers: ApiKeyProviderSummary[];
  accounts: OfficialAccountSummary[];
  pendingKey: (key: string) => boolean;
  usageByProvider: Record<string, UsageView>;
  connectivityByProvider: Record<string, ConnectivityView>;
  testingProviderId: string | null;
  onLaunchApp: (preset: LaunchPresetSummary) => void;
  onLaunchCli: (preset: LaunchPresetSummary) => void;
  onToggleModel: (
    preset: LaunchPresetSummary,
    providerId: string,
    upstreamModelId: string,
  ) => void;
  onAddProvider: (preset: LaunchPresetSummary, providerId: string) => void;
  onAddProviderToNewWorkspace: (providerId: string) => void;
  onRemoveProvider: (preset: LaunchPresetSummary, providerId: string) => void;
  onDefaultModelChange: (preset: LaunchPresetSummary, modelId: string) => void;
  onLoginProviderChange: (
    preset: LaunchPresetSummary,
    providerId: string,
  ) => void;
  onOpenUtility: (view: UtilityDrawerView, preset?: LaunchPresetSummary) => void;
  onOpenAddProvider: () => void;
  onOpenInstances: (preset: LaunchPresetSummary) => void;
  onOpenDefaultCodex: () => void;
  onOpenCodexHome: (preset: LaunchPresetSummary) => void;
  onTestProvider: (providerId: string) => void;
  onReorderChannels: (
    preset: LaunchPresetSummary,
    providerIds: string[],
  ) => void;
  onSelectInstance: (preset: LaunchPresetSummary) => Promise<boolean>;
  onCreateInstance: () => Promise<string | null>;
  instances: RuntimeInstanceSummary[];
  onStopInstance: (instance: RuntimeInstanceSummary) => void;
}) {
  const { exposuresByProvider, exposureErrorsByProvider, deleteManagedWorkspace } = useAppState();
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [switching, setSwitching] = useState(false);
  const [deleteTarget, setDeleteTarget] = useState<LaunchPresetSummary | null>(null);
  const workspaces = presets.filter(
    (candidate) => candidate.entryKind === "managed_app",
  ).sort(compareWorkspaceOrder);
  const preset =
    workspaces.find((candidate) => candidate.id === selectedId) ??
    workspaces.find((candidate) => candidate.isPreferred) ??
    workspaces[0];
  const availableChannelChoices: WorkspaceChannelChoice[] = providers
    .filter((provider) => !preset?.providerIds.includes(provider.id))
    .sort((left, right) => compareModelSortText(left.name, right.name))
    .map((provider) => {
      const report = connectivityByProvider[provider.id];
      const protocolConfirmed =
        report?.wireApi === "responses" || report?.wireApi === "chat_completions";
      const exposedCount = (exposuresByProvider[provider.id] ?? []).filter((model) => model.exposed && isWorkspaceSelectableModelId(model.upstreamModelId)).length;
      const exposureError = exposureErrorsByProvider[provider.id];
      const selectable = provider.configured && protocolConfirmed && !exposureError && exposedCount > 0;
      return {
        id: provider.id,
        label: provider.name,
        host: providerHost(provider.baseUrl),
        statusLabel: report
          ? connectivityLabels[report.status]
          : provider.configured
            ? "未检测"
            : "未配置",
        discoveredModelCount: exposedCount,
        protocolLabel: protocolText(report?.wireApi),
        usageLabel: usageObservationLabel(
          provider.transportMode,
          usageByProvider[provider.id],
        ),
        disabled: !selectable,
        disabledReason: !provider.configured
          ? "请先补全此模型渠道的 API Key。"
          : !protocolConfirmed
            ? "请先检测此模型渠道并确认支持的协议。"
            : exposureError
              ? exposureError
              : exposuresByProvider[provider.id] === undefined
                ? "正在读取模型透出状态。"
                : exposedCount === 0
                  ? "请先在模型渠道中选择要透出的模型。"
                  : undefined,
      };
    });
  const creating = pendingKey("preset:create");
  const deleting = Boolean(deleteTarget && pendingKey(`workspace:delete:${deleteTarget.id}`));
  const deletionReason = (workspace: LaunchPresetSummary | null | undefined) => {
    if (!workspace) return "当前没有可删除的隔离空间。";
    if (instances.some((instance) => instance.launchPresetId === workspace.id && isActiveInstanceState(instance.state))) return "请先停止此空间的所有 App 和 CLI 实例，并核验已退出，再删除空间。";
    if (pendingKey(`workspace:delete:${workspace.id}`)) return "正在删除此隔离空间，请等待完成。";
    if (pendingKey(`preset:${workspace.id}`) || creating || switching) return "正在更新隔离空间，请稍候。";
    return undefined;
  };
  const instanceTabs: WorkspaceInstanceTab[] = workspaces.map((workspace) => {
    const running = instances.some(
      (instance) =>
        instance.launchPresetId === workspace.id &&
        isManagedAppTarget(instance.launchTarget) &&
        isActiveInstanceState(instance.state),
    );
    const deleteReason = deletionReason(workspace);
    return {
      id: workspace.id,
      label: workspace.name,
      running,
      deleteDisabled: Boolean(deleteReason),
      deleteReason,
    };
  });
  const atWorkspaceLimit = workspaces.length >= 5;
  const instanceCardProps = {
    instances: instanceTabs,
    selectedInstanceId: preset?.id ?? "",
    createDisabled: creating || atWorkspaceLimit || deleting || switching,
    tabsDisabled: creating || deleting || switching,
    onDeleteInstance: (id: string) => {
      const target = workspaces.find((candidate) => candidate.id === id);
      if (!target || deletionReason(target)) return;
      setDeleteTarget(target);
    },
    createReason: deleting || switching
      ? "正在处理隔离空间，请稍候。"
      : creating
      ? "正在新增隔离空间，请稍候。"
      : atWorkspaceLimit
        ? "最多只能创建 5 个隔离空间。"
        : undefined,
    onSelectInstance: (id: string) => {
      const next = workspaces.find((candidate) => candidate.id === id);
      if (!next || switching || deleting || id === preset?.id) return;
      const previousId = preset?.id ?? null;
      setSelectedId(id);
      setSwitching(true);
      void onSelectInstance(next).then((saved) => {
        if (!saved) setSelectedId(previousId);
      }).catch(() => setSelectedId(previousId)).finally(() => setSwitching(false));
    },
    onCreateInstance: () => {
      void onCreateInstance().then((id) => {
        if (id) setSelectedId(id);
      });
    },
  };
  const missingInstanceReason = preset
    ? undefined
    : "请先配置模型渠道和模型。";
  const releaseHold = "初版暂不开放。";
  const openingHome = pendingKey("codex-home");
  const openingDefault = pendingKey("default-codex");
  const spaceActions = (
    <>
      <HintedButton
        className="icon-button control-icon control-detail"
        type="button"
        size="md"
        title="实例详情"
        aria-label="实例详情"
        disabled
        disabledReason={releaseHold}
      >
        <Info size={18} aria-hidden="true" />
      </HintedButton>
      <HintedButton
        className="icon-button control-icon control-requests"
        type="button"
        size="md"
        title="请求记录"
        aria-label="请求记录"
        disabled
        disabledReason={releaseHold}
      >
        <ScrollText size={18} aria-hidden="true" />
      </HintedButton>
      <HintedButton
        className="icon-button control-icon control-folder"
        type="button"
        size="md"
        title="打开隔离目录"
        aria-label="打开隔离目录"
        disabled={!preset || openingHome}
        disabledReason={
          missingInstanceReason ??
          (openingHome ? "正在打开隔离目录。" : undefined)
        }
        onClick={() => {
          if (preset) onOpenCodexHome(preset);
        }}
      >
        <FolderOpen size={18} aria-hidden="true" />
      </HintedButton>
      <HintedButton
        className="icon-button control-icon control-default"
        type="button"
        size="md"
        title="打开默认 Codex"
        aria-label="打开默认 Codex"
        disabled={openingDefault}
        disabledReason={
          openingDefault ? "正在打开默认 Codex。" : undefined
        }
        onClick={onOpenDefaultCodex}
      >
        <AppWindow size={18} aria-hidden="true" />
      </HintedButton>
      {deleteTarget ? (
        <WorkspaceDeleteDialog
          key={deleteTarget.id}
          workspace={deleteTarget}
          busy={deleting}
          blockedReason={deleting ? undefined : deletionReason(deleteTarget)}
          onClose={() => { if (!deleting) setDeleteTarget(null); }}
          onConfirm={async () => {
            const result = await deleteManagedWorkspace(deleteTarget);
            setSelectedId(result.selectedPresetId);
            setDeleteTarget(null);
          }}
        />
      ) : null}
    </>
  );
  if (!preset)
    return (
      <>
        <ChannelWorkspaceCard
          configured={false}
          rows={[]}
          choices={availableChannelChoices}
          modelOptions={[]}
          defaultModelId=""
          loginProviderId=""
          loginProviderOptions={[]}
          loading={creating}
          launchDisabled
          launchReason="请先配置模型渠道和模型。"
          configureDisabled={creating}
          configureReason={creating ? "正在配置隔离工作区，请稍候。" : undefined}
          cliDisabled
          cliReason="请先配置模型渠道和模型。"
          testingChannelId={null}
          onOpenChannels={() => onOpenUtility("credentials")}
          onToggleModel={() => undefined}
          moreActions={spaceActions}
          onAddChannel={onAddProviderToNewWorkspace}
          onOpenAddProvider={onOpenAddProvider}
          onRemoveChannel={() => undefined}
          onDefaultModelChange={() => undefined}
          onLoginProviderChange={() => undefined}
          onLaunch={() => undefined}
          onLaunchCli={() => undefined}
          onTestChannel={() => undefined}
          onReorderChannels={() => undefined}
          {...instanceCardProps}
        />
      </>
    );
  const visibleModels = sortedManagedModels(
    preset.models.filter(
      (model) =>
        isWorkspaceSelectableModelId(model.upstreamModelId) &&
        isWorkspaceSelectableModelId(model.publicModelId),
    ),
    preset.providerIds,
  );
  const configuredDefaultModelId =
    preset.models.find(
      (model) =>
        isWorkspaceSelectableModelId(model.upstreamModelId) &&
        isWorkspaceSelectableModelId(model.publicModelId),
    )?.publicModelId ?? visibleModels[0]?.publicModelId ?? "";
  const modelCount = visibleModels.length;
  const cardLoading = pendingKey(`preset:${preset.id}`) || creating || deleting || switching;
  const runningApp = instances.find(
    (instance) =>
      instance.launchPresetId === preset.id &&
      isManagedAppTarget(instance.launchTarget) &&
      isActiveInstanceState(instance.state),
  );
  const stoppingApp = Boolean(
    runningApp &&
      (runningApp.state === "stopping" ||
        pendingKey("instance:" + runningApp.id)),
  );
  const exposureReadError = preset.providerIds.some((id) => exposureErrorsByProvider[id]);
  const launchReason = exposureReadError
    ? "模型透出状态读取失败，请在模型渠道中重试读取。"
    : cardLoading
    ? "正在更新工作区，请稍候。"
    : modelCount === 0
      ? "请先为这个启动入口选择至少一个模型。"
      : preset.models.some((model) => model.sourceKind === "official")
        ? "官方模型当前暂时关闭。"
        : undefined;
  const loginReason =
    !preset.loginProviderId ||
    !preset.providerIds.includes(preset.loginProviderId)
      ? "请选择 API 登录渠道。"
      : undefined;
  const appReason = launchReason;
  const configureDisabled = cardLoading;
  const configureReason = cardLoading
    ? "正在更新工作区，请稍候。"
    : undefined;
  const discoveredOptions = buildDiscoveredModelOptions(
    preset.providerIds.map((id) => {
      const source = providers.find((candidate) => candidate.id === id);
      return {
        providerId: id,
        providerName: source?.name ?? "模型渠道",
        models: selectableProviderModels(
          connectivityByProvider[id]?.models ?? [],
        ).filter((model) => isModelExposed(exposuresByProvider, id, model.upstreamModelId)).map((model) => ({
          upstreamModelId: model.upstreamModelId,
          displayName: model.displayName,
          ownedBy: model.ownedBy,
        })),
      };
    }),
  );
  const rows: WorkspaceChannelRow[] = preset.providerIds.map((providerId) => {
    const provider = providers.find((candidate) => candidate.id === providerId);
    const report = connectivityByProvider[providerId];
    const health =
      !provider || !provider.configured
        ? {
            label: provider ? "未配置" : "已移除",
            tone: "not_run",
            recover:
              provider && !provider.configured
                ? ("unauthorized" as const)
                : null,
          }
        : !report
          ? { label: "未检测", tone: "not_run", recover: null }
          : {
              label: connectivityLabels[report.status],
              tone: report.status,
              recover:
                report.status === "unauthorized"
                  ? ("unauthorized" as const)
                  : report.status === "unreachable"
                    ? ("unreachable" as const)
                    : null,
            };
    const models = visibleModels.filter(
      (model) =>
        model.sourceKind === "api_provider" && model.sourceId === providerId,
    );
    const probedModels = report?.models ?? [];
    const imageModels = probedModels.filter((model) =>
      isImageGenerationModelId(model.upstreamModelId),
    );
    return {
      id: providerId,
      label: provider?.name ?? "已移除渠道",
      host: provider ? providerHost(provider.baseUrl) : "—",
      statusLabel: health.label,
      statusTone: health.tone,
      modelCount: models.length,
      modelNames: models
        .slice(0, 3)
        .map((model) => model.displayName || model.publicModelId),
      isLogin: preset.loginProviderId === providerId,
      usageLabel: usageObservationLabel(
        provider?.transportMode,
        usageByProvider[providerId],
      ),
      protocolLabel: protocolText(report?.wireApi),
      discoveredCount: visibleProviderModels(report?.models ?? []).length,
      hiddenRoleModelCount: hiddenCodexRoleModelCount(report?.models ?? []),
      imageModelCount: imageModels.length,
      imageModelLabels: imageModels
        .slice(0, 3)
        .map((model) => model.displayName || model.upstreamModelId),
      selectedImageModels: preset.models
        .filter(
          (model) =>
            model.sourceKind === "api_provider" &&
            model.sourceId === providerId &&
            isImageGenerationModelId(model.upstreamModelId),
        )
        .map((model) => ({
          upstreamModelId: model.upstreamModelId,
          label: model.displayName || model.upstreamModelId,
        })),
      probed: Boolean(report) || models.length > 0,
      modelSelectionHint: exposureErrorsByProvider[providerId]
        ?? (exposuresByProvider[providerId] === undefined
          ? "正在读取模型透出状态。"
          : !(exposuresByProvider[providerId] ?? []).some((model) => model.exposed)
            ? "请先在模型渠道中选择要透出的模型。"
            : undefined),
      modelChoices: [
        ...(report?.wireApi === "responses" || report?.wireApi === "chat_completions"
          ? discoveredOptions.filter((option) => option.providerId === providerId).map((option) => ({
              upstreamModelId: option.upstreamModelId,
              label: option.displayLabel,
              detail: [option.upstreamModelId, option.ownedBy, option.duplicate ? "同名模型已按渠道区分" : ""].filter(Boolean).join(" · "),
              selected: models.some((model) => model.upstreamModelId === option.upstreamModelId),
            }))
          : []),
        ...models.filter((model) => !discoveredOptions.some((option) => option.providerId === providerId && option.upstreamModelId === model.upstreamModelId)
          || !(report?.wireApi === "responses" || report?.wireApi === "chat_completions"))
          .map((model) => ({
            upstreamModelId: model.upstreamModelId,
            label: model.displayName || model.upstreamModelId,
            detail: "当前空间已保存的模型；取消勾选可将它移出空间。",
            selected: true,
          })),
      ],
      recover: health.recover,
    };
  });
  const modelOptions = visibleModels.map((model) => ({
    id: model.publicModelId,
    label: qualifiedModelLabel(model, providers, accounts),
  }));
  const loginProviderOptions = preset.providerIds.flatMap((id) => {
    const provider = providers.find((candidate) => candidate.id === id);
    return provider ? [{ id: provider.id, label: provider.name }] : [];
  });
  return (
    <>
      <ChannelWorkspaceCard
        configured
        rows={rows}
        choices={availableChannelChoices}
        modelOptions={modelOptions}
        defaultModelId={configuredDefaultModelId}
        loginProviderId={preset.loginProviderId ?? ""}
        loginProviderOptions={loginProviderOptions}
        loading={cardLoading}
        launchDisabled={Boolean(appReason ?? loginReason)}
        launchReason={appReason ?? loginReason}
        configureDisabled={configureDisabled}
        configureReason={configureReason}
        cliDisabled={Boolean(launchReason || loginReason)}
        cliReason={launchReason ?? loginReason}
        appRunning={Boolean(runningApp)}
        stopBusy={stoppingApp}
        onStop={() => {
          if (runningApp) onStopInstance(runningApp);
        }}
        testingChannelId={testingProviderId}
        onOpenChannels={() => onOpenUtility("credentials")}
        onToggleModel={(providerId, upstreamModelId) =>
          onToggleModel(preset, providerId, upstreamModelId)
        }
        moreActions={spaceActions}
        onAddChannel={(providerId) => onAddProvider(preset, providerId)}
        onOpenAddProvider={onOpenAddProvider}
        onRemoveChannel={(providerId) => onRemoveProvider(preset, providerId)}
        onDefaultModelChange={(modelId) =>
          onDefaultModelChange(preset, modelId)
        }
        onLoginProviderChange={(providerId) =>
          onLoginProviderChange(preset, providerId)
        }
        onLaunch={() => onLaunchApp(preset)}
        onLaunchCli={() => onLaunchCli(preset)}
          onTestChannel={onTestProvider}
        onReorderChannels={(providerIds) =>
          onReorderChannels(preset, providerIds)
        }
        {...instanceCardProps}
      />
    </>
  );
}

/**
 * @component IsolatedAppInfo
 * @description 独立工作区页面。
 */
export function IsolatedAppInfo(props: Parameters<typeof IsolatedAppBody>[0]) {
  return (
    <main className="dashboard-home">
      <IsolatedAppBody {...props} />
    </main>
  );
}
