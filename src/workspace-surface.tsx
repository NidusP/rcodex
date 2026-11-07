import { Fragment, type ReactNode } from "react";
import {
  AppWindow,
  ChevronDown,
  CirclePlus,
  Play,
  Square,
  Terminal,
  Trash2,
} from "lucide-react";
import { Button, Menu, Select, Tabs, Tooltip } from "@mantine/core";
import { ChannelRowList } from "./channel-sort-list";
import { HintedButton } from "./components/hinted-button";
import { AppUpdateButton, GatewayRestartButton } from "./components/title-bar";
import "./workspace-surface.css";

export type WorkspaceChannelRow = {
  id: string;
  label: string;
  host: string;
  statusLabel: string;
  statusTone: string;
  modelCount: number;
  modelNames: string[];
  protocolLabel: string;
  discoveredCount: number;
  hiddenRoleModelCount: number;
  imageModelCount: number;
  imageModelLabels: string[];
  selectedImageModels: Array<{
    upstreamModelId: string;
    label: string;
  }>;
  probed: boolean;
  modelSelectionHint?: string;
  modelChoices: Array<{
    upstreamModelId: string;
    label: string;
    detail: string;
    selected: boolean;
  }>;
  isLogin: boolean;
  usageLabel: string;
  recover: "unauthorized" | "unreachable" | null;
};

export type WorkspaceChannelChoice = {
  id: string;
  label: string;
  host: string;
  statusLabel: string;
  discoveredModelCount: number;
  protocolLabel: string;
  usageLabel: string;
  disabled?: boolean;
  disabledReason?: string;
};

export type WorkspaceInstanceTab = {
  id: string;
  label: string;
  running?: boolean;
  /** 该空间当前不可删除时为 true，例如仍有活动实例。 */
  deleteDisabled?: boolean;
  /** 不可删除的原因，悬停垃圾桶时展示。 */
  deleteReason?: string;
};

type Option = { id: string; label: string };

export function usageObservationLabel(
  transportMode: string | undefined,
  usage:
    | { today: { requests: number }; lastRequestAt: number | null }
    | undefined,
) {
  if (!usage || usage.lastRequestAt === null) return "未观测";
  return `今日 ${usage.today.requests.toLocaleString()} 次`;
}

/**
 * @component ChannelAddMenu
 * @description 从已有渠道中选择一个加入当前工作区。
 */
function ChannelAddMenu({
  choices,
  disabled,
  disabledReason,
  onSelect,
  onCreate,
  label = "添加渠道",
}: {
  choices: WorkspaceChannelChoice[];
  disabled: boolean;
  disabledReason?: string;
  onSelect: (id: string) => void;
  onCreate?: () => void;
  label?: string;
}) {
  const unavailable = disabled || (choices.length === 0 && !onCreate);
  const hint = choices.length
    ? "[选择一个未加入的渠道]"
    : onCreate
      ? "[或新建一个模型渠道]"
      : "[没有可加入的渠道]";
  return (
    <Menu
      position="bottom-end"
      withinPortal
      disabled={unavailable}
      classNames={{ dropdown: "channel-add-menu" }}
    >
      <Menu.Target>
        <Button
          className="channel-add-trigger"
          type="button"
          variant="default"
          aria-label={label}
          disabled={unavailable}
          title={
            unavailable
              ? (disabledReason ??
                (choices.length ? "当前不能加入渠道。" : "没有可加入的渠道"))
              : "选择一个未加入的渠道"
          }
        >
          <span className="channel-add-label">
            <CirclePlus size={18} aria-hidden="true" />
            <b>{label}</b>
            <small>{hint}</small>
          </span>
          <ChevronDown
            className="channel-add-chevron"
            size={16}
            aria-hidden="true"
          />
        </Button>
      </Menu.Target>
      <Menu.Dropdown aria-label="未加入的渠道">
        {choices.length > 0 ? <Menu.Label>待分配上游</Menu.Label> : null}
        {choices.map((choice) => {
          // 未透出模型、未配置等原因导致暂不可加入的渠道仍列出，
          // 但状态列显示不可用，悬停展示具体原因。
          const statusTone = choice.disabled ? "不可用" : choice.statusLabel;
          const item = (
            <Menu.Item
              disabled={choice.disabled}
              onClick={() => {
                if (!choice.disabled) onSelect(choice.id);
              }}
              rightSection={<small data-tone={statusTone}>{statusTone}</small>}
            >
              <span>
                <i data-tone={statusTone} aria-hidden="true" />
                {choice.label}
              </span>
            </Menu.Item>
          );
          if (!choice.disabled || !choice.disabledReason) {
            return <Fragment key={choice.id}>{item}</Fragment>;
          }
          return (
            <Tooltip
              key={choice.id}
              label={choice.disabledReason}
              multiline
              maw={280}
              withArrow
              openDelay={300}
              zIndex={450}
              events={{ hover: true, focus: true, touch: true }}
              classNames={{ tooltip: "control-tooltip" }}
            >
              <span
                className="disabled-hint channel-choice-blocked"
                tabIndex={0}
                aria-label={"当前不可用：" + choice.disabledReason}
              >
                {item}
              </span>
            </Tooltip>
          );
        })}
        {onCreate ? (
          <>
            {choices.length > 0 ? <Menu.Divider /> : null}
            <Menu.Item
              onClick={onCreate}
              leftSection={<CirclePlus size={15} aria-hidden="true" />}
            >
              新建模型渠道
            </Menu.Item>
          </>
        ) : null}
      </Menu.Dropdown>
    </Menu>
  );
}

const EMPTY_SELECT_VALUE = "__rcodex_empty__";

/**
 * @component CalibrationSelect
 * @description 控制带上的模型或渠道选择器，标题和说明由 Mantine Select 显示。
 */
function CalibrationSelect({
  label,
  hint,
  value,
  placeholder,
  options,
  disabled,
  disabledReason,
  onChange,
}: {
  label: string;
  hint: string;
  value: string;
  placeholder: string;
  options: Option[];
  disabled: boolean;
  disabledReason?: string;
  onChange: (value: string) => void;
}) {
  const data = options.map((option) => ({
    value: option.id === "" ? EMPTY_SELECT_VALUE : option.id,
    label: option.label,
  }));
  const selected = value === "" ? EMPTY_SELECT_VALUE : value;
  const hasSelected = data.some((option) => option.value === selected);
  const control = (
    <Select
      className="calibration-field"
      label={label}
      description={hint}
      data={data}
      value={hasSelected ? selected : null}
      onChange={(next) => {
        if (next === null || next === EMPTY_SELECT_VALUE) onChange("");
        else onChange(next);
      }}
      placeholder={placeholder}
      disabled={disabled || data.length === 0}
      allowDeselect={false}
      withCheckIcon={false}
      comboboxProps={{
        withinPortal: true,
        position: "top-start",
        middlewares: { flip: true, shift: true },
        zIndex: 400,
      }}
    />
  );
  if (!disabled || !disabledReason) return control;
  return (
    <Tooltip
      label={disabledReason}
      multiline
      maw={280}
      withArrow
      openDelay={300}
      zIndex={450}
      position="top"
      events={{ hover: true, focus: true, touch: true }}
    >
      <span className="disabled-hint calibration-disabled">{control}</span>
    </Tooltip>
  );
}

/**
 * @component WorkspaceTabs
 * @description 使用 Mantine Tabs 切换固定名称的隔离空间；每个 tab 名称后带各自的删除入口。
 */
function WorkspaceTabs({
  instances,
  selectedInstanceId,
  createDisabled,
  createReason,
  tabsDisabled = false,
  onDeleteInstance,
  onSelectInstance,
  onCreateInstance,
}: {
  instances: WorkspaceInstanceTab[];
  selectedInstanceId: string;
  createDisabled: boolean;
  createReason?: string;
  tabsDisabled?: boolean;
  onDeleteInstance: (id: string) => void;
  onSelectInstance: (id: string) => void;
  onCreateInstance: () => void;
}) {
  return (
    <div className="workspace-tabs">
      <Tabs
        value={selectedInstanceId || null}
        onChange={(value) => {
          if (value) onSelectInstance(value);
        }}
      >
        <Tabs.List aria-label="隔离空间">
          {instances.map((instance) => (
            <Tabs.Tab
              key={instance.id}
              value={instance.id}
              disabled={tabsDisabled}
              rightSection={
                <span className="workspace-tab-tools">
                  <Tooltip
                    label={instance.deleteReason ?? `永久删除${instance.label}`}
                    withArrow
                    openDelay={300}
                  >
                    <span
                      className={
                        "workspace-tab-delete" +
                        (instance.deleteDisabled ? " is-disabled" : "")
                      }
                      role="button"
                      aria-label={
                        instance.deleteDisabled
                          ? (instance.deleteReason ?? "当前不能删除该隔离空间")
                          : `删除${instance.label}`
                      }
                      aria-disabled={instance.deleteDisabled || undefined}
                      onClick={(event) => {
                        event.stopPropagation();
                        event.preventDefault();
                        if (!instance.deleteDisabled) {
                          onDeleteInstance(instance.id);
                        }
                      }}
                    >
                      <Trash2 size={12} aria-hidden="true" />
                    </span>
                  </Tooltip>
                  {instance.running ? (
                    <span
                      className="workspace-tabs-running"
                      aria-label="运行中"
                      title="运行中"
                    >
                      ●
                    </span>
                  ) : null}
                </span>
              }
            >
              {instance.label}
            </Tabs.Tab>
          ))}
        </Tabs.List>
      </Tabs>
      <Tooltip
        label={createReason ?? "新增隔离空间"}
        withArrow
        openDelay={300}
        disabled={!createDisabled && !createReason}
      >
        <span className="workspace-tabs-create-wrap">
          <Button
            className="workspace-tabs-create"
            type="button"
            variant="default"
            size="xs"
            disabled={createDisabled}
            onClick={onCreateInstance}
            aria-label="新增隔离空间"
          >
            <CirclePlus size={15} aria-hidden="true" />
          </Button>
        </span>
      </Tooltip>
    </div>
  );
}

/**
 * @component ChannelWorkspaceCard
 * @description 工作区主卡片：品牌行、当前实例渠道和启动控制带。
 */
export function ChannelWorkspaceCard({
  configured,
  rows,
  choices,
  modelOptions,
  defaultModelId,
  loginProviderId,
  loginProviderOptions,
  loading,
  launchDisabled,
  launchReason,
  configureDisabled,
  configureReason,
  lockNotice,
  cliDisabled,
  cliReason,
  testingChannelId,
  onAddChannel,
  onOpenAddProvider,
  onRemoveChannel,
  onDefaultModelChange,
  onLoginProviderChange,
  onLaunch,
  onLaunchCli,
  appRunning = false,
  onStop,
  stopBusy = false,
  onTestChannel,
  onToggleModel,
  onReorderChannels,
  onOpenChannels,
  moreActions,
  instances,
  selectedInstanceId,
  createDisabled,
  createReason,
  tabsDisabled = false,
  onDeleteInstance,
  onSelectInstance,
  onCreateInstance,
}: {
  configured: boolean;
  rows: WorkspaceChannelRow[];
  choices: WorkspaceChannelChoice[];
  modelOptions: Option[];
  defaultModelId: string;
  loginProviderId: string;
  loginProviderOptions: Option[];
  loading: boolean;
  launchDisabled: boolean;
  launchReason?: string;
  configureDisabled: boolean;
  configureReason?: string;
  lockNotice?: string;
  cliDisabled: boolean;
  cliReason?: string;
  testingChannelId: string | null;
  onAddChannel: (providerId: string) => void;
  onOpenAddProvider: () => void;
  onRemoveChannel: (providerId: string) => void;
  onDefaultModelChange: (modelId: string) => void;
  onLoginProviderChange: (providerId: string) => void;
  onLaunch: () => void;
  onLaunchCli: () => void;
  appRunning?: boolean;
  onStop?: () => void;
  stopBusy?: boolean;
  onTestChannel: (providerId: string) => void;
  onToggleModel: (providerId: string, upstreamModelId: string) => void;
  onReorderChannels: (providerIds: string[]) => void;
  onOpenChannels: () => void;
  moreActions?: ReactNode;
  instances: WorkspaceInstanceTab[];
  selectedInstanceId: string;
  createDisabled: boolean;
  createReason?: string;
  tabsDisabled?: boolean;
  onDeleteInstance: (id: string) => void;
  onSelectInstance: (id: string) => void;
  onCreateInstance: () => void;
}) {
  return (
    <article
      className={`runtime-target-card${loading ? " is-loading" : ""}`}
      aria-busy={loading}
    >
      {loading && (
        <div className="runtime-target-loading" role="status">
          <span className="loading-spinner" aria-hidden="true" />
          <span>正在更新工作区…</span>
        </div>
      )}
      <div className="runtime-target-heading">
        <div className="space-brand-row">
          <div className="space-brand-cluster">
            <strong className="space-brand">R Codex</strong>
            {import.meta.env.DEV ? <span className="space-brand-dev">dev</span> : null}
            <GatewayRestartButton />
          </div>
          <div className="space-brand-tools">
            <AppUpdateButton />
            <HintedButton
              className="space-channel-entry"
              type="button"
              onClick={onOpenChannels}
            >
              <span>模型渠道</span>
            </HintedButton>
          </div>
        </div>
      </div>
      <div className="runtime-target-route">
        <div className="runtime-target-route-heading channel-section-heading">
          <div className="channel-section-title">
            <WorkspaceTabs
              instances={instances}
              selectedInstanceId={selectedInstanceId}
              createDisabled={createDisabled}
              createReason={createReason}
              tabsDisabled={tabsDisabled}
              onDeleteInstance={onDeleteInstance}
              onSelectInstance={onSelectInstance}
              onCreateInstance={onCreateInstance}
            />
            {lockNotice ? (
              <small className="channel-lock-note">{lockNotice}</small>
            ) : null}
          </div>
          {configured ? (
            <ChannelAddMenu
              choices={choices}
              disabled={configureDisabled}
              disabledReason={configureReason}
              onSelect={onAddChannel}
            />
          ) : null}
        </div>
        {configured ? (
          rows.length === 0 ? (
            <div className="empty-workspace-guidance">
              <p className="empty-detail">
                尚未加入模型渠道。使用上方“添加渠道”选择已有渠道，或新建一个模型渠道。
              </p>
              <ChannelAddMenu
                choices={choices}
                disabled={configureDisabled}
                disabledReason={configureReason}
                label="选择或新建模型渠道"
                onSelect={onAddChannel}
                onCreate={onOpenAddProvider}
              />
            </div>
          ) : (
            <ChannelRowList
              rows={rows}
              onReorder={onReorderChannels}
              actions={{
                configureDisabled,
                configureReason,
                testingChannelId,
                onTestChannel,
                onToggleModel,
                onRemoveChannel,
              }}
            />
          )
        ) : (
          <div className="empty-workspace-guidance">
            <p className="empty-detail">
              尚未配置工作区。请选择已有模型渠道，或新建一个模型渠道。
            </p>
            <ChannelAddMenu
              choices={choices}
              disabled={configureDisabled}
              disabledReason={configureReason}
              label="选择或新建模型渠道"
              onSelect={onAddChannel}
              onCreate={onOpenAddProvider}
            />
          </div>
        )}
      </div>
      <div className="control-strip">
        {configured && (
          <div className="calibration-grid">
            <CalibrationSelect
              label="默认调用模型"
              hint="启动时的主模型。"
              value={defaultModelId}
              placeholder="未选择"
              options={modelOptions}
              disabled={configureDisabled}
              disabledReason={configureReason}
              onChange={onDefaultModelChange}
            />
            <CalibrationSelect
              label="API 登录渠道"
              hint="会话鉴权来源。"
              value={loginProviderId}
              placeholder="选择 API 登录渠道"
              options={loginProviderOptions}
              disabled={configureDisabled}
              disabledReason={configureReason}
              onChange={onLoginProviderChange}
            />
          </div>
        )}
        <div className="control-actions">
          <HintedButton
            className="icon-button control-icon control-cli"
            type="button"
            size="md"
            title="以当前配置模型运行codex cli"
            aria-label="以当前配置模型运行codex cli"
            disabled={!configured || cliDisabled}
            disabledReason={cliReason ?? "请先配置模型渠道和模型。"}
            onClick={onLaunchCli}
          >
            <Terminal size={18} aria-hidden="true" />
          </HintedButton>
          <HintedButton
            className={
              "icon-button control-icon control-app" +
              (appRunning ? " is-running" : "")
            }
            type="button"
            size="md"
            title={appRunning ? "停止当前app实例" : "启动当前app实例"}
            aria-label={appRunning ? "停止当前app实例" : "启动当前app实例"}
            disabled={
              appRunning ? stopBusy || !onStop : !configured || launchDisabled
            }
            disabledReason={
              appRunning
                ? "正在停止当前 App 实例。"
                : (launchReason ?? "请先配置模型渠道和模型。")
            }
            onClick={appRunning ? onStop : onLaunch}
          >
            <Play className="control-app-play" size={18} aria-hidden="true" />
            {appRunning ? (
              <Square
                className="control-app-stop"
                size={16}
                aria-hidden="true"
              />
            ) : null}
          </HintedButton>
          {moreActions}
        </div>
      </div>
    </article>
  );
}
