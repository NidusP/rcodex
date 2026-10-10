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
  onRemoveProvider,
  onDefaultModelChange,
  onLoginProviderChange,
  onOpenUtility,
  onOpenDefaultCodex,
  onOpenCodexHome,
  onSyncResult,
  onTestProvider,
  onReorderChannels,
  onSelectInstance,
  onCreateInstance,
  onRenameInstance,
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
  onRemoveProvider: (preset: LaunchPresetSummary, providerId: string) => void;
  onDefaultModelChange: (preset: LaunchPresetSummary, modelId: string) => void;
  onLoginProviderChange: (
    preset: LaunchPresetSummary,
    providerId: string,
  ) => void;
  onOpenUtility: (view: UtilityDrawerView, preset?: LaunchPresetSummary) => void;
  onOpenInstances: (preset: LaunchPresetSummary) => void;
  onOpenDefaultCodex: () => void;
  onOpenCodexHome: (preset: LaunchPresetSummary) => void;
  onSyncResult: (message: string) => void;
  onTestProvider: (providerId: string) => void;
  onReorderChannels: (
    preset: LaunchPresetSummary,
    providerIds: string[],
  ) => void;
  onSelectInstance: (preset: LaunchPresetSummary) => void;
  onCreateInstance: (preset?: LaunchPresetSummary) => Promise<string | null>;
  onRenameInstance: (preset: LaunchPresetSummary, name: string) => void;
  instances: RuntimeInstanceSummary[];
  onStopInstance: (instance: RuntimeInstanceSummary) => void;
}) {
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const workspaces = presets.filter(
    (candidate) => candidate.entryKind === "managed_app",
  );
  const preset =
    workspaces.find((candidate) => candidate.id === selectedId) ??
    workspaces.find((candidate) => candidate.isPreferred) ??
    workspaces[0];
  const creating = pendingKey("preset:create");
  const instanceTabs: WorkspaceInstanceTab[] = workspaces.map((workspace, index) => {
    return {
      id: workspace.id,
      label:
        workspace.name === "R Codex"
          ? index === 0
            ? "独立隔离空间"
            : `独立隔离空间 ${index + 1}`
          : workspace.name,
    };
  });
  const instanceCardProps = {
    instances: instanceTabs,
    selectedInstanceId: preset?.id ?? "",
    createDisabled: creating,
    createReason: creating ? "正在新增实例，请稍候。" : undefined,
    onSelectInstance: (id: string) => {
      const next = workspaces.find((candidate) => candidate.id === id);
      if (!next) return;
      setSelectedId(id);
      onSelectInstance(next);
    },
    onCreateInstance: () => {
      void onCreateInstance(preset).then((id) => {
        if (id) setSelectedId(id);
      });
    },
    onRenameInstance: (id: string, name: string) => {
      const next = workspaces.find((candidate) => candidate.id === id);
      if (next) onRenameInstance(next, name);
    },
    onRenameBlocked: () =>
      onSyncResult("当前不能修改实例名称。"),
    onRenameInvalid: () =>
      onSyncResult("实例名称不能为空，最长 80 字节，也不能包含控制字符。"),
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
    </>
  );
  if (!preset)
    return (
      <>
        <ChannelWorkspaceCard
          configured={false}
          rows={[]}
          choices={[]}
          modelOptions={[]}
          defaultModelId=""
          loginProviderId=""
          loginProviderOptions={[]}
          loading={creating}
          launchDisabled
          launchReason="请先配置模型渠道和模型。"
          configureDisabled={false}
          cliDisabled
          cliReason="请先配置模型渠道和模型。"
          testingChannelId={null}
          onOpenChannels={() => onOpenUtility("credentials")}
          onToggleModel={() => undefined}
          moreActions={spaceActions}
          onAddChannel={() => undefined}
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
  const cardLoading = pendingKey(`preset:${preset.id}`) || creating;
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
  const launchReason = cardLoading
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
        ).map((model) => ({
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
      probed: Boolean(report),
      modelChoices: discoveredOptions
        .filter((option) => option.providerId === providerId)
        .map((option) => ({
          upstreamModelId: option.upstreamModelId,
          label: option.displayLabel,
          detail: [
            option.upstreamModelId,
            option.ownedBy,
            option.duplicate ? "同名模型已按渠道区分" : "",
          ]
            .filter(Boolean)
            .join(" · "),
          selected: preset.models.some(
            (model) =>
              model.sourceKind === "api_provider" &&
              model.sourceId === providerId &&
              model.upstreamModelId === option.upstreamModelId,
          ),
        })),
      recover: health.recover,
    };
  });
  const choices: WorkspaceChannelChoice[] = providers
    .filter((provider) => !preset.providerIds.includes(provider.id))
    .sort((left, right) => compareModelSortText(left.name, right.name))
    .map((provider) => {
      const report = connectivityByProvider[provider.id];
      return {
        id: provider.id,
        label: provider.name,
        host: providerHost(provider.baseUrl),
        statusLabel: report
          ? connectivityLabels[report.status]
          : provider.configured
            ? "未检测"
            : "未配置",
        discoveredModelCount: visibleProviderModels(report?.models ?? [])
          .length,
        protocolLabel: protocolText(report?.wireApi),
        usageLabel: usageObservationLabel(
          provider.transportMode,
          usageByProvider[provider.id],
        ),
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
        choices={choices}
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
