import { type ReactNode } from "react";
import {
  AppWindow,
  ChevronDown,
  CirclePlus,
  Play,
  Square,
  Terminal,
} from "lucide-react";
import { Button, Menu, Select, Tooltip } from "@mantine/core";
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
};

export type WorkspaceInstanceTab = {
  id: string;
  label: string;
  renameDisabled?: boolean;
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
}: {
  choices: WorkspaceChannelChoice[];
  disabled: boolean;
  disabledReason?: string;
  onSelect: (id: string) => void;
}) {
  const unavailable = disabled || choices.length === 0;
  const hint = choices.length ? "[选择一个未加入的渠道]" : "[没有可加入的渠道]";
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
          aria-label="添加渠道"
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
            <b>添加渠道</b>
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
        <Menu.Label>待分配上游</Menu.Label>
        {choices.map((choice) => (
          <Menu.Item
            key={choice.id}
            onClick={() => onSelect(choice.id)}
            rightSection={<small>{choice.statusLabel}</small>}
          >
            <span>
              <i data-tone={choice.statusLabel} aria-hidden="true" />
              {choice.label}
            </span>
          </Menu.Item>
        ))}
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
  onSelectInstance: (id: string) => void;
  onCreateInstance: () => void;
  onRenameInstance: (id: string, name: string) => void;
  onRenameBlocked?: () => void;
  onRenameInvalid?: () => void;
}) {
  const instanceName =
    instances.find((instance) => instance.id === selectedInstanceId)?.label ||
    "独立隔离空间";
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
            <strong>{instanceName}</strong>
            {lockNotice ? (
              <small className="channel-lock-note">{lockNotice}</small>
            ) : null}
          </div>
          {configured && (
            <ChannelAddMenu
              choices={choices}
              disabled={configureDisabled}
              disabledReason={configureReason}
              onSelect={onAddChannel}
            />
          )}
        </div>
        {configured ? (
          rows.length === 0 ? (
            <div className="empty-workspace-guidance">
              <p className="empty-detail">
                尚未加入模型渠道。使用上方“添加渠道”选择已有渠道，或新建一个模型渠道。
              </p>
              <HintedButton
                className="secondary-action"
                type="button"
                onClick={onOpenChannels}
              >
                选择或新建模型渠道
              </HintedButton>
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
            <HintedButton
              className="secondary-action"
              type="button"
              onClick={onOpenChannels}
            >
              选择或新建模型渠道
            </HintedButton>
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
