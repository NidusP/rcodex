import {
  AlertTriangle,
  ArrowRight,
  CheckCircle2,
  FolderOpen,
  GitBranch,
  Monitor,
  RefreshCw,
  Square,
  Trash2,
  X,
} from "lucide-react";
import { useState } from "react";
import { ControlDialog } from "../control-dialog";
import {
  type Provider,
  type GatewayStatus,
  type InstanceRequestDiagnostic,
  type ConversationSummary,
  formatTime,
  tokenCount,
  instanceDiagnosticHint,
  isGatewayPreset,
  gatewayStateLabel,
  modeDescription,
  routePathLabel,
} from "../../app-model";
import {
  type LaunchPresetSummary,
  type LaunchTarget,
  type OfficialAccountSummary,
  type RuntimeInstanceSummary,
  isActiveInstanceState,
  isOfficialCredential,
  launchTargetLabel,
  credentialKindLabel,
  isManagedAppTarget,
} from "../../codex-control";
import {
  providerHost,
  isInternalCodexModelId,
  providerForModel,
  sortedManagedModels,
  sourceLabelForModel,
} from "../model-labels";
import { HintedButton } from "../hinted-button";
import { useAppState } from "../app-state";
import "./runtime-panels.css";

/**
 * @component RoutePreview
 * @description 启动方案的路由摘要预览。
 */
export function RoutePreview({
  preset,
  providers,
  accounts,
  compact = false,
}: {
  preset: LaunchPresetSummary;
  providers: Provider[];
  accounts: OfficialAccountSummary[];
  compact?: boolean;
}) {
  const models = sortedManagedModels(
    (preset.models ?? []).filter(
      (model) =>
        !isInternalCodexModelId(model.upstreamModelId) &&
        !isInternalCodexModelId(model.publicModelId),
    ),
    preset.providerIds,
  );
  const hasAutomaticReview = models.some((model) => {
    const source = providerForModel(model, providers, accounts);
    return Boolean(
      source &&
      "transportMode" in source &&
      source.transportMode !== "gateway_chat_completions",
    );
  });
  const sources = Array.from(
    new Set(
      models.map((model) => sourceLabelForModel(model, providers, accounts)),
    ),
  );
  const firstProvider = models
    .map((model) => providerForModel(model, providers, accounts))
    .find((source): source is Provider =>
      Boolean(source && "baseUrl" in source),
    );
  const destination = isGatewayPreset(preset)
    ? "按公共模型精确路由"
    : firstProvider
      ? providerHost(firstProvider.baseUrl)
      : (sources[0] ?? "真实模型渠道");
  return (
    <section
      className={`route-preview ${compact ? "compact" : ""}`}
      aria-label={`${preset.name} 模型路由预览`}
    >
      <div className="route-preview-heading">
        <div>
          <strong>{compact ? "当前路由" : "本次模型路由"}</strong>
          <small>
            {models.length} 个模型 · {sources.length} 个模型渠道
          </small>
        </div>
        <span
          className="mode-pill"
          title={modeDescription(preset.transportMode ?? "gateway_passthrough")}
        >
          {isGatewayPreset(preset) ? "Gateway 统一路由" : "单渠道直连"}
        </span>
      </div>
      <div className="route-flow" aria-label={routePathLabel(preset)}>
        <span className="route-flow-node">受管理 Codex</span>
        <ArrowRight size={14} aria-hidden="true" />
        <span className="route-flow-node">
          {isGatewayPreset(preset)
            ? "本机 Gateway"
            : (sources[0] ?? "模型渠道")}
        </span>
        <ArrowRight size={14} aria-hidden="true" />
        <span className="route-flow-node">{destination}</span>
      </div>
      {isGatewayPreset(preset) && hasAutomaticReview ? (
        <p className="detail-note">
          <strong>自动审核</strong> · 随当前 Responses 模型所属系列路由
        </p>
      ) : null}
      {models.length > 0 ? (
        <div className="route-model-list">
          {models.slice(0, compact ? 4 : 20).map((model) => (
            <div
              className="route-model-row"
              key={`${model.sourceKind}:${model.sourceId}:${model.publicModelId}`}
            >
              <span className="route-model-id">{model.publicModelId}</span>
              <ArrowRight size={12} aria-hidden="true" />
              <span>{model.displayName}</span>
              <small>
                {sourceLabelForModel(model, providers, accounts)} · 上游{" "}
                {model.upstreamModelId}
              </small>
            </div>
          ))}
          {models.length > (compact ? 4 : 20) ? (
            <small className="route-model-more">
              另有 {models.length - (compact ? 4 : 20)} 个模型未展开
            </small>
          ) : null}
        </div>
      ) : (
        <p className="empty-detail">
          尚未配置模型路由；启动前不会向任何模型渠道发送请求。
        </p>
      )}
    </section>
  );
}

/**
 * @component LaunchCodexDialog
 * @description 确认启动隔离 Codex CLI 或 App。
 */
export function LaunchCodexDialog({
  preset,
  preferredTarget,
  busy,
  onCancel,
  onConfirm,
}: {
  preset: LaunchPresetSummary;
  preferredTarget: LaunchTarget;
  busy: boolean;
  onCancel: () => void;
  onConfirm: (target: LaunchTarget, workingDirectory: string) => void;
}) {
  const [workingDirectory, setWorkingDirectory] = useState("");
  const isCliTarget = preferredTarget === "cli";
  const cliLabel = "启动隔离 Codex CLI";
  return (
    <ControlDialog
      onClose={onCancel}
      closeDisabled={busy}
      size={520}
      label={"启动 " + preset.name}
      className="control-dialog-launch"
    >
      <section className="launch-dialog" aria-labelledby="launch-codex-title">
        <div className="drawer-heading">
          <h2 id="launch-codex-title">启动 {preset.name}</h2>
        </div>
        <p id="launch-codex-copy" className="launch-dialog-copy">
          启动方案只会应用到 R Codex 的隔离 Codex CLI；不会读取或修改用户 Codex
          配置。启动前可指定本次工作目录。
        </p>
        <div className="launch-summary" aria-label="本次 Codex 启动方案">
          <div>
            <span>凭据</span>
            <strong>{preset.credentialLabel}</strong>
          </div>
          <div>
            <span>路由</span>
            <strong>本机 Gateway</strong>
          </div>
        </div>
        {isCliTarget ? (
          <label
            className="launch-directory-field"
            title="只设置本次 Codex CLI 的工作目录。"
          >
            <span>
              <FolderOpen size={15} aria-hidden="true" /> 工作目录
            </span>
            <input
              value={workingDirectory}
              onChange={(event) => setWorkingDirectory(event.target.value)}
              placeholder="留空使用配置默认目录或用户主目录"
              disabled={busy}
            />
          </label>
        ) : null}
        <div className="confirm-dialog-actions">
          <HintedButton
            className="secondary-action"
            type="button"
            disabled={busy}
            onClick={onCancel}
          >
            取消
          </HintedButton>
          {isCliTarget ? (
            <HintedButton
              className="primary-action"
              type="button"
              disabled={busy}
              onClick={() => onConfirm(preferredTarget, workingDirectory)}
            >
              {busy ? "正在启动…" : cliLabel}
            </HintedButton>
          ) : null}
        </div>
      </section>
    </ControlDialog>
  );
}

/**
 * @component LaunchPresetDetailDrawer
 * @description 查看启动方案的技术详情。
 */
export function LaunchPresetDetailDrawer({
  preset,
  providers,
  accounts,
  onClose,
}: {
  preset: LaunchPresetSummary;
  providers: Provider[];
  accounts: OfficialAccountSummary[];
  onClose: () => void;
}) {
  return (
    <div
      className="drawer-layer"
      role="presentation"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <aside
        className="detail-drawer"
        role="dialog"
        aria-modal="true"
        aria-labelledby="preset-detail-title"
      >
        <div className="drawer-heading">
          <div>
            <p className="eyebrow">CODEX LAUNCH PLAN DETAIL</p>
            <h2 id="preset-detail-title">{preset.name}</h2>
          </div>
          <HintedButton
            className="icon-button circular-button"
            type="button"
            title="关闭详情"
            aria-label="关闭启动方案详情"
            onClick={onClose}
          >
            <X size={17} aria-hidden="true" />
          </HintedButton>
        </div>
        <div className="drawer-status">
          <span className="provider-type-pill">
            {credentialKindLabel(preset.credentialKind, "官方 Codex 登录")}
          </span>
          <span className="mode-pill">Gateway 统一路由</span>
        </div>
        <dl className="detail-list">
          <div>
            <dt>凭据</dt>
            <dd>{preset.credentialLabel}</dd>
          </div>
          <div>
            <dt>工作目录</dt>
            <dd>启动 CLI 时选择</dd>
          </div>
          <div>
            <dt>模型数量</dt>
            <dd>
              {
                preset.models.filter(
                  (model) =>
                    !isInternalCodexModelId(model.upstreamModelId) &&
                    !isInternalCodexModelId(model.publicModelId),
                ).length
              }
            </dd>
          </div>
        </dl>
        <RoutePreview
          preset={preset}
          providers={providers}
          accounts={accounts}
        />
      </aside>
    </div>
  );
}

const instanceStateLabels: Record<RuntimeInstanceSummary["state"], string> = {
  launching: "正在启动",
  running: "运行中",
  stopping: "正在停止",
  orphaned: "等待核验",
  degraded: "状态降级",
  exited: "已退出",
  failed: "启动失败",
};

/**
 * @component RuntimeInstanceList
 * @description 受管理 Codex 实例列表与生命周期操作。
 */
export function RuntimeInstanceList({
  instances,
  presets,
  providers,
  accounts,
  gateway,
  focusIds,
  pendingKey,
  onStop,
  onRestart,
  onReconcile,
  onDelete,
  diagnosticsById,
  summariesById,
  loadingId,
  onLoadDiagnostics,
  onClearDiagnostics,
}: {
  instances: RuntimeInstanceSummary[];
  presets: LaunchPresetSummary[];
  providers: Provider[];
  accounts: OfficialAccountSummary[];
  gateway: GatewayStatus;
  focusIds: string[];
  pendingKey: (key: string) => boolean;
  onStop: (instance: RuntimeInstanceSummary) => void;
  onRestart: (instance: RuntimeInstanceSummary) => void;
  onReconcile: (instance: RuntimeInstanceSummary) => void;
  onDelete: (instance: RuntimeInstanceSummary) => void;
  diagnosticsById: Record<string, InstanceRequestDiagnostic[]>;
  summariesById: Record<string, ConversationSummary[]>;
  loadingId: string | null;
  onLoadDiagnostics: (instanceId: string) => void;
  onClearDiagnostics: (instance: RuntimeInstanceSummary) => void;
}) {
  if (instances.length === 0)
    return (
      <div className="empty-state">
        <Monitor size={22} aria-hidden="true" />
        <strong>没有运行记录</strong>
        <span>
          这里仅展示由 R Codex 启动并负责生命周期管理的隔离 Codex CLI 与 App。
        </span>
      </div>
    );
  const focused = new Set(focusIds);
  return (
    <div className="instance-list">
      {instances.map((instance) => {
        const active = isActiveInstanceState(instance.state);
        const pending = pendingKey(`instance:${instance.id}`);
        const requestEvents = diagnosticsById[instance.id];
        const summaries = summariesById[instance.id];
        const preset = instance.launchPresetId
          ? presets.find(
              (candidate) => candidate.id === instance.launchPresetId,
            )
          : undefined;
        const gatewayLabel =
          preset && isGatewayPreset(preset)
            ? gatewayStateLabel(gateway.state)
            : "直连 Provider";
        return (
          <article
            className={`instance-row ${focused.has(instance.id) ? "is-focused" : ""}`}
            id={`instance-${instance.id}`}
            key={instance.id}
          >
            <div className="instance-heading">
              <span className={`instance-state ${instance.state}`}>
                <i aria-hidden="true" />
                {instanceStateLabels[instance.state]}
              </span>
              <strong>{instance.credentialLabel}</strong>
              <span className="provider-type-pill">
                {isOfficialCredential(instance.credentialKind)
                  ? "官方"
                  : "API Key"}
              </span>
            </div>
            <div className="instance-scan">
              <span>{launchTargetLabel(instance.launchTarget)}</span>
              <span>
                {instance.launchedAt
                  ? formatTime(instance.launchedAt)
                  : "尚未记录启动时间"}
              </span>
              <span>{instance.workspacePath ?? "默认目录"}</span>
              <span>{gatewayLabel}</span>
            </div>
            <details className="instance-technical">
              <summary>技术详情</summary>
              <dl>
                <div>
                  <dt>类型</dt>
                  <dd>{launchTargetLabel(instance.launchTarget)}</dd>
                </div>
                <div>
                  <dt>实例</dt>
                  <dd>{instance.id}</dd>
                </div>
                <div>
                  <dt>启动方案</dt>
                  <dd>{preset?.name ?? "修订快照"}</dd>
                </div>
                <div>
                  <dt>模型</dt>
                  <dd>{preset ? `${preset.models.length} 个` : "—"}</dd>
                </div>
                <div>
                  <dt>配置修订</dt>
                  <dd>{instance.launchConfigurationRevisionId}</dd>
                </div>
                <div>
                  <dt>凭据修订</dt>
                  <dd>{instance.credentialRevisionId ?? "—"}</dd>
                </div>
                <div>
                  <dt>PID</dt>
                  <dd>{instance.pid ?? "—"}</dd>
                </div>
                <div>
                  <dt>工作目录</dt>
                  <dd>{instance.workspacePath ?? "默认"}</dd>
                </div>
                <div>
                  <dt>启动时间</dt>
                  <dd>
                    {instance.launchedAt
                      ? formatTime(instance.launchedAt)
                      : "—"}
                  </dd>
                </div>
              </dl>
            </details>
            <div className="instance-route-summary">
              <div className="instance-route-copy">
                <GitBranch size={15} aria-hidden="true" />
                <div>
                  <strong>{gatewayLabel}</strong>
                  <small>{instanceDiagnosticHint(instance, preset)}</small>
                </div>
              </div>
              {preset ? (
                <RoutePreview
                  preset={preset}
                  providers={providers}
                  accounts={accounts}
                  compact
                />
              ) : (
                <span className="muted">
                  无法找到对应启动方案，仅展示本次运行快照。
                </span>
              )}
            </div>
            {!active ? (
              <div className="instance-exit-note">
                <CheckCircle2 size={14} aria-hidden="true" />
                <span>{instanceDiagnosticHint(instance, preset)}</span>
                {instance.exitedAt ? (
                  <time>{formatTime(instance.exitedAt)}</time>
                ) : null}
              </div>
            ) : null}
            {instance.startupLog ||
            instance.state === "failed" ||
            instance.state === "orphaned" ||
            instance.state === "degraded" ? (
              <details className="instance-log">
                <summary>查看启动日志</summary>
                <pre>
                  {instance.startupLog ||
                    "暂无启动器日志。请重新点击启动，并保留随后打开的 Terminal 窗口以查看 Codex 自身输出。"}
                </pre>
              </details>
            ) : null}
            {requestEvents ? (
              <section
                className="detail-block instance-diagnostics"
                aria-label={`${instance.id} 请求诊断`}
              >
                <div className="detail-block-heading">
                  <strong>请求级协议诊断</strong>
                  <span>
                    {requestEvents.length} 条 ·{" "}
                    {
                      new Set(requestEvents.map((event) => event.providerId))
                        .size
                    }{" "}
                    个 Provider
                  </span>
                </div>
                {requestEvents.length === 0 ? (
                  <p className="empty-detail">该实例尚无 Gateway 请求记录。</p>
                ) : (
                  <div className="diagnostic-list">
                    {requestEvents.slice(0, 20).map((event) => {
                      const providerName =
                        providers.find(
                          (provider) => provider.id === event.providerId,
                        )?.name ?? event.providerId;
                      const totalTokens =
                        event.totalTokens ??
                        (event.inputTokens !== null &&
                        event.outputTokens !== null
                          ? event.inputTokens + event.outputTokens
                          : null);
                      return (
                        <div className="diagnostic-row" key={event.id}>
                          <span
                            className={`outcome ${event.outcome === "success" ? "success" : "failed"}`}
                          >
                            {event.outcome === "success"
                              ? "成功"
                              : event.outcome}
                          </span>
                          <div>
                            <strong>{event.publicModelId}</strong>
                            <small>
                              {providerName} · {event.upstreamModelId} ·{" "}
                              {formatTime(event.createdAt)}
                            </small>
                            <small>
                              {event.wireApi} ·{" "}
                              {event.responseContentType ?? "无 Content-Type"}
                              {event.stream ? " · 流式" : " · 非流式"} · Token{" "}
                              {tokenCount(totalTokens)}（入{" "}
                              {tokenCount(event.inputTokens)} / 出{" "}
                              {tokenCount(event.outputTokens)}）
                            </small>
                          </div>
                          <div className="diagnostic-meta">
                            <strong>{event.httpStatus ?? "网络错误"}</strong>
                            <small>
                              {event.firstByteMs === null
                                ? "首字节 —"
                                : `首字节 ${event.firstByteMs}ms`}{" "}
                              ·{" "}
                              {event.totalLatencyMs === null
                                ? "总耗时 —"
                                : `${event.totalLatencyMs}ms`}
                            </small>
                          </div>
                          {event.errorCode && (
                            <code title="脱敏后的真实上游地址与限长错误摘要。Codex 看到的 127.0.0.1 只是本机 Gateway。">
                              {event.errorCode}
                              {event.errorDetail
                                ? `：${event.errorDetail}`
                                : ""}
                            </code>
                          )}
                        </div>
                      );
                    })}
                  </div>
                )}
              </section>
            ) : null}
            {summaries ? (
              <section
                className="detail-block instance-diagnostics"
                aria-label={`${instance.id} 对话摘要`}
              >
                <div className="detail-block-heading">
                  <strong>脱敏对话摘要</strong>
                  <span>{summaries.length} 条</span>
                </div>
                {summaries.length === 0 ? (
                  <p className="empty-detail">该实例尚无对话摘要。</p>
                ) : (
                  <div className="diagnostic-list">
                    {summaries.slice(0, 20).map((summary) => (
                      <div className="diagnostic-row" key={summary.id}>
                        <span className="outcome success">摘要</span>
                        <div>
                          <strong>
                            {summary.sessionId
                              ? `会话 ${summary.sessionId.slice(0, 12)}…`
                              : "未识别会话"}
                          </strong>
                          <small>
                            {formatTime(summary.createdAt)} · 输入{" "}
                            {summary.inputLength} · 输出 {summary.outputLength}
                          </small>
                        </div>
                        <div className="diagnostic-meta">
                          <small>
                            {summary.toolCallTypes.length
                              ? `工具 ${summary.toolCallTypes.join(", ")}`
                              : "无工具调用"}
                          </small>
                        </div>
                      </div>
                    ))}
                  </div>
                )}
              </section>
            ) : null}
            <div className="instance-actions">
              <HintedButton
                className="secondary-action"
                type="button"
                title="读取该实例的脱敏请求诊断和对话摘要"
                disabled={loadingId === instance.id}
                disabledReason="正在读取实例诊断。"
                onClick={() => onLoadDiagnostics(instance.id)}
              >
                <RefreshCw size={14} aria-hidden="true" />
                {loadingId === instance.id
                  ? "读取中"
                  : requestEvents
                    ? "刷新诊断"
                    : "查看诊断"}
              </HintedButton>
              {requestEvents && (
                <HintedButton
                  className="secondary-action"
                  type="button"
                  title="清除该实例的本地诊断和对话摘要"
                  disabled={pending}
                  disabledReason="正在处理实例诊断。"
                  onClick={() => onClearDiagnostics(instance)}
                >
                  清除诊断
                </HintedButton>
              )}
              {instance.state === "orphaned" ||
              instance.state === "degraded" ||
              instance.state === "stopping" ? (
                <HintedButton
                  className="secondary-action"
                  type="button"
                  title="重新核验该进程是否仍在运行"
                  disabled={pending}
                  disabledReason="正在核验实例。"
                  onClick={() => onReconcile(instance)}
                >
                  <RefreshCw size={14} aria-hidden="true" />
                  重新核验
                </HintedButton>
              ) : null}
              {active ? (
                <>
                  <HintedButton
                    className="secondary-action"
                    type="button"
                    title="停止并重新启动该隔离实例，使用新的路由快照"
                    disabled={pending || instance.state === "stopping"}
                    disabledReason="实例正在停止或重启。"
                    onClick={() => onRestart(instance)}
                  >
                    <RefreshCw size={14} aria-hidden="true" />
                    重启实例
                  </HintedButton>
                  <HintedButton
                    className="danger-action"
                    type="button"
                    title="显式停止该受管理实例"
                    disabled={pending || instance.state === "stopping"}
                    disabledReason="实例正在停止。"
                    onClick={() => onStop(instance)}
                  >
                    <Square size={13} fill="currentColor" aria-hidden="true" />
                    停止实例
                  </HintedButton>
                </>
              ) : null}
              {instance.state === "exited" || instance.state === "failed" ? (
                <HintedButton
                  className="icon-button provider-icon-action danger"
                  type="button"
                  title="删除已结束的实例记录"
                  aria-label="删除实例记录"
                  disabled={pending}
                  disabledReason="正在删除实例记录。"
                  onClick={() => onDelete(instance)}
                >
                  <Trash2 size={15} aria-hidden="true" />
                </HintedButton>
              ) : null}
            </div>
          </article>
        );
      })}
    </div>
  );
}

/**
 * @component ConfirmResourceDeleteDialog
 * @description 确认删除实例或其他受管理资源。
 */
export function ConfirmResourceDeleteDialog({
  title,
  copy,
  busy,
  onCancel,
  onConfirm,
}: {
  title: string;
  copy: string;
  busy: boolean;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  return (
    <ControlDialog
      onClose={onCancel}
      closeDisabled={busy}
      size={430}
      label={title}
      className="control-dialog-confirm"
    >
      <section
        className="confirm-dialog"
        role="alertdialog"
        aria-labelledby="resource-delete-title"
        aria-describedby="resource-delete-copy"
      >
        <AlertTriangle size={22} aria-hidden="true" />
        <div>
          <h2 id="resource-delete-title">{title}</h2>
          <p id="resource-delete-copy">{copy}</p>
        </div>
        <div className="confirm-dialog-actions">
          <HintedButton
            className="secondary-action"
            type="button"
            disabled={busy}
            onClick={onCancel}
          >
            取消
          </HintedButton>
          <HintedButton
            className="danger-action"
            type="button"
            disabled={busy}
            onClick={onConfirm}
          >
            {busy ? "正在删除…" : "确认删除"}
          </HintedButton>
        </div>
      </section>
    </ControlDialog>
  );
}

/**
 * @component RuntimeOverlays
 * @description 启动、实例删除和清空等运行时叠层。
 */
export function RuntimeOverlays() {
  const {
    providers,
    accounts,
    showClearInstances,
    setShowClearInstances,
    launchCandidate,
    setLaunchCandidate,
    launchPreferredTarget,
    selectedPresetDetails,
    setSelectedPresetDetails,
    instanceDeleteCandidate,
    setInstanceDeleteCandidate,
    instanceClearCandidate,
    setInstanceClearCandidate,
    pendingKey,
    clearInstanceDiagnostics,
    launchPreset,
    deleteInstance,
    clearSafeInstances,
  } = useAppState();
  return (
    <>
      {launchCandidate && (
        <LaunchCodexDialog
          preset={launchCandidate}
          preferredTarget={launchPreferredTarget}
          busy={pendingKey(`preset:${launchCandidate.id}`)}
          onCancel={() => setLaunchCandidate(null)}
          onConfirm={(target, workingDirectory) =>
            void launchPreset(launchCandidate, workingDirectory, target)
          }
        />
      )}

      {selectedPresetDetails && (
        <LaunchPresetDetailDrawer
          preset={selectedPresetDetails}
          providers={providers}
          accounts={accounts}
          onClose={() => setSelectedPresetDetails(null)}
        />
      )}

      {showClearInstances && (
        <ConfirmResourceDeleteDialog
          title="清除结束实例及其隔离数据？"
          copy="此操作不可恢复：会删除所有已退出或启动失败实例的隔离数据；仍在运行或被锁定的实例会保留。"
          busy={pendingKey("instances:clear")}
          onCancel={() => setShowClearInstances(false)}
          onConfirm={() => void clearSafeInstances()}
        />
      )}

      {instanceDeleteCandidate && (
        <ConfirmResourceDeleteDialog
          title={`删除隔离 ${isManagedAppTarget(instanceDeleteCandidate.launchTarget) ? "App" : "CLI"} 实例数据？`}
          copy="此操作不可恢复：会删除该实例的隔离数据、诊断记录和实例准入密钥，不会影响模型渠道、启动方案或用户默认 Codex。"
          busy={pendingKey(`instance:${instanceDeleteCandidate.id}`)}
          onCancel={() => setInstanceDeleteCandidate(null)}
          onConfirm={() => void deleteInstance(instanceDeleteCandidate)}
        />
      )}

      {instanceClearCandidate && (
        <ConfirmResourceDeleteDialog
          title="清除该实例的诊断？"
          copy="将永久删除该实例的请求级协议诊断和脱敏对话摘要，不会停止实例或修改默认 Codex 配置。"
          busy={pendingKey(`instance:${instanceClearCandidate.id}`)}
          onCancel={() => setInstanceClearCandidate(null)}
          onConfirm={() =>
            void clearInstanceDiagnostics(instanceClearCandidate)
          }
        />
      )}
    </>
  );
}
