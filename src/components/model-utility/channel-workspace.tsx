import { useState } from "react";
import {
  AlertTriangle,
  Layers3,
  Pencil,
  RefreshCw,
  Trash2,
  X,
} from "lucide-react";
import {
  type Provider,
  type ProviderConnectivityReport,
  type ProviderUsageSummary,
  type UsagePeriod,
  balanceLabel,
  balanceHint,
  formatTime,
  maskKey,
  tokenCount,
  tokenTotal,
  checkLabel,
  successRate,
} from "../../app-model";
import {
  isDirectTransport,
  runningProviderLockReason,
} from "../../codex-control";
import {
  catalogModelKind,
  catalogModelPurpose,
  connectivityLabels,
  hiddenCodexRoleModelCount,
  hiddenCodexRoleModelNote,
  imageGenerationModelNote,
  protocolText,
  providerHost,
  selectableProviderModels,
} from "../model-labels";
import { usageObservationLabel } from "../../workspace-surface";
import { AddChannelDialog } from "./add-channel-dialog";
import { HintedButton } from "../hinted-button";
import { ControlDialog } from "../control-dialog";
import { ControlDrawer } from "./drawer-layout";
import { useAppState } from "../app-state";
import "./channel-workspace.css";

/**
 * @component StatusBadge
 * @description 展示模型渠道连通性或首选状态。
 */
function StatusBadge({
  report,
}: {
  provider: Provider;
  report?: ProviderConnectivityReport;
}) {
  const label = report ? connectivityLabels[report.status] : "未检测";
  const tone = report?.status ?? "not_run";
  const hint = report
    ? `最近检测：${connectivityLabels[report.status]}${report.errorCode ? `，错误码 ${report.errorCode}` : ""}。`
    : "尚未执行连通性检测。";
  return (
    <span className={`status-badge ${tone}`} title={hint}>
      <i aria-hidden="true" />
      {label}
    </span>
  );
}

/**
 * @component UsagePeriodCard
 * @description 展示某一时间窗口的本地用量摘要。
 */
function UsagePeriodCard({
  label,
  period,
  observed = true,
}: {
  label: string;
  period: UsagePeriod;
  observed?: boolean;
}) {
  return (
    <div className="usage-period">
      <strong>{label}</strong>
      <span>
        {observed
          ? `${period.requests.toLocaleString()} 请求 · ${successRate(period)} 成功`
          : "未观测"}
      </span>
      <small>
        {observed
          ? `输入 ${tokenCount(period.inputTokens)} · 输出 ${tokenCount(period.outputTokens)} Token`
          : "没有经过本机 Gateway 的记录"}
      </small>
    </div>
  );
}

/**
 * @component ConnectivityPanel
 * @description 渠道详情里的连通性检测结果。
 */
function ConnectivityPanel({
  provider,
  report,
}: {
  provider: Provider;
  report?: ProviderConnectivityReport;
}) {
  if (!report)
    return (
      <p className="empty-detail">
        尚未检测。检测会读取模型列表，并可能发送一次最小文本请求。
      </p>
    );
  const capability =
    "模型渠道只负责上游连接与模型发现；工作区请求统一经本机 Gateway 路由，上游协议由渠道能力决定。复杂能力取决于所选上游。";
  const visibleModels = selectableProviderModels(report.models);
  const hiddenModelCount = hiddenCodexRoleModelCount(report.models);
  const imageNote = imageGenerationModelNote(
    report.models
      .filter(
        (model) =>
          catalogModelKind(model.upstreamModelId) === "image_generation",
      )
      .map((model) => model.displayName || model.upstreamModelId),
  );
  return (
    <section
      className={`detail-block connectivity-detail ${report.status}`}
      aria-label={`${provider.name} 连通性检测`}
    >
      <div className="detail-block-heading">
        <strong>连通性</strong>
        <span>{connectivityLabels[report.status]}</span>
      </div>
      <div className="check-grid">
        <span>
          模型列表 <b>{checkLabel(report.modelsCheck)}</b>
        </span>
        <span>
          基础文本 <b>{checkLabel(report.textCheck)}</b>
        </span>
      </div>
      {visibleModels.length > 0 && (
        <div className="discovered-model-summary">
          <strong>已发现 {visibleModels.length} 个可选模型</strong>
          <div>
            {visibleModels.slice(0, 8).map((model) => (
              <span key={model.upstreamModelId} title={model.upstreamModelId}>
                {model.displayName}
              </span>
            ))}
            {visibleModels.length > 8 && (
              <span>+{visibleModels.length - 8}</span>
            )}
          </div>
          {hiddenCodexRoleModelNote(hiddenModelCount) ? (
            <small>{hiddenCodexRoleModelNote(hiddenModelCount)}</small>
          ) : null}
        </div>
      )}
      {visibleModels.length === 0 &&
      hiddenCodexRoleModelNote(hiddenModelCount) ? (
        <p className="detail-note">
          {hiddenCodexRoleModelNote(hiddenModelCount)}
        </p>
      ) : null}
      {imageNote ? <p className="detail-note">{imageNote}</p> : null}
      <p>{capability}</p>
      <small>
        检测于 {formatTime(report.checkedAt)}
        {report.errorCode ? ` · ${report.errorCode}` : ""}
      </small>
    </section>
  );
}

/**
 * @component UsagePanel
 * @description 渠道详情里的本地用量和上游余额。
 */
function UsagePanel({
  provider,
  summary,
}: {
  provider: Provider;
  summary?: ProviderUsageSummary;
}) {
  if (!summary)
    return (
      <section className="detail-block">
        <div className="detail-block-heading">
          <strong>用量</strong>
          <span>不可用</span>
        </div>
        <p className="empty-detail">
          本地用量暂时不可读取；不会影响模型渠道或 Gateway。
        </p>
      </section>
    );
  return (
    <section className="detail-block" aria-label={`${provider.name} 用量信息`}>
      <div className="detail-block-heading">
        <strong>用量与余额</strong>
        <span>本地统计</span>
      </div>
      <div className="usage-grid">
        {(() => {
          const observed =
            !isDirectTransport(provider.transportMode) &&
            summary.lastRequestAt !== null;
          return (
            <>
              <UsagePeriodCard
                label="今天"
                period={summary.today}
                observed={observed}
              />
              <UsagePeriodCard
                label="本周"
                period={summary.week}
                observed={observed}
              />
              <UsagePeriodCard
                label="本月"
                period={summary.month}
                observed={observed}
              />
            </>
          );
        })()}
      </div>
      <div className="usage-meta">
        <span>
          估算费用：
          {summary.month.estimatedCostUsd === null
            ? "未配置价格"
            : `$${summary.month.estimatedCostUsd.toFixed(2)}`}
        </span>
        <span title={balanceHint(summary.upstreamBalance)}>
          上游余额：{balanceLabel(summary.upstreamBalance)}
        </span>
        <span>
          最近活动：
          {summary.lastRequestAt === null
            ? "暂无请求"
            : formatTime(summary.lastRequestAt)}
        </span>
      </div>
    </section>
  );
}

/**
 * @component ProviderModelCatalogDrawer
 * @description 查看某个模型渠道最近一次探测到的模型，并说明哪些能在工作区勾选。
 */
export function ProviderModelCatalogDrawer({
  provider,
  report,
  onClose,
  onRefresh,
  refreshing,
  refreshDisabledReason,
}: {
  provider: Provider;
  report?: ProviderConnectivityReport;
  onClose: () => void;
  onRefresh: () => void;
  refreshing: boolean;
  refreshDisabledReason?: string;
}) {
  const kindOrder = { chat: 0, image_generation: 1, codex_role: 2 };
  const models = (report?.models ?? []).slice().sort((left, right) => {
    const leftKind = catalogModelKind(left.upstreamModelId);
    const rightKind = catalogModelKind(right.upstreamModelId);
    const byKind = kindOrder[leftKind] - kindOrder[rightKind];
    if (byKind !== 0) return byKind;
    return (left.displayName || left.upstreamModelId).localeCompare(
      right.displayName || right.upstreamModelId,
    );
  });
  const chatCount = models.filter(
    (model) => catalogModelKind(model.upstreamModelId) === "chat",
  ).length;
  const heldBackCount = models.length - chatCount;
  return (
    <ControlDrawer
      opened={true}
      onClose={onClose}
      title={provider.name + " 支持的模型"}
      size={480}
      zIndex={80}
    >
      {!report ? (
        <div className="provider-model-catalog-empty">
          <p className="empty-detail">
            尚未读取此渠道的模型目录。请点击更新渠道
          </p>
          <HintedButton
            className="icon-button"
            type="button"
            size="md"
            title="更新渠道"
            aria-label="更新渠道"
            disabled={Boolean(refreshDisabledReason)}
            disabledReason={refreshDisabledReason}
            onClick={onRefresh}
          >
            {refreshing ? (
              <span className="loading-spinner" aria-hidden="true" />
            ) : (
              <RefreshCw size={16} aria-hidden="true" />
            )}
          </HintedButton>
        </div>
      ) : (
        <>
          <div className="drawer-status">
            <StatusBadge provider={provider} report={report} />
            <span className="mode-pill">{chatCount} 个对话模型</span>
            {heldBackCount > 0 ? (
              <span className="mode-pill">
                {heldBackCount} 个不进入工作区勾选
              </span>
            ) : null}
            <span className="mode-pill">{protocolText(report.wireApi)}</span>
          </div>
          <p className="drawer-intro">
            这是最近一次检测保存的 /models 快照。查看不会访问上游、读取 API Key
            或修改工作区。对话模型可以在工作区勾选，启动后显示在 Codex
            模型目录。图像生成模型（例如
            grok-imagine-image-2.0）走单独的生图接口，不会显示在工作区勾选中。Codex
            系统角色用于自动审核或子代理，也不会进入普通模型目录。
          </p>
          {models.length > 0 ? (
            <div
              className="provider-model-catalog-list"
              aria-label={provider.name + " 支持的模型列表"}
            >
              {models.map((model) => (
                <div
                  className="provider-model-catalog-row"
                  key={model.upstreamModelId}
                >
                  <strong>{model.displayName || model.upstreamModelId}</strong>
                  <code>{model.upstreamModelId}</code>
                  <small>{catalogModelPurpose(model.upstreamModelId)}</small>
                  {model.ownedBy && <small>{model.ownedBy}</small>}
                </div>
              ))}
            </div>
          ) : (
            <p className="empty-detail">最近一次检测没有读到模型</p>
          )}
          <small className="provider-model-catalog-meta">
            {"快照时间：" + formatTime(report.checkedAt)}
            {report.errorCode ? " · 最近错误：" + report.errorCode : ""}
          </small>
        </>
      )}
    </ControlDrawer>
  );
}

/**
 * @component DeleteProviderDialog
 * @description 确认删除模型渠道及其系统凭据和本地记录。
 */
export function DeleteProviderDialog({
  provider,
  busy,
  onCancel,
  onConfirm,
}: {
  provider: Provider;
  busy: boolean;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  return (
    <ControlDialog
      onClose={onCancel}
      closeDisabled={busy}
      size={430}
      label={"删除 " + provider.name}
      className="control-dialog-confirm"
    >
      <section
        className="confirm-dialog"
        role="alertdialog"
        aria-labelledby="delete-provider-title"
        aria-describedby="delete-provider-copy"
      >
        <AlertTriangle size={22} aria-hidden="true" />
        <div>
          <h2 id="delete-provider-title">删除 {provider.name}？</h2>
          <p id="delete-provider-copy">
            系统凭据、本地用量、诊断记录和该模型渠道的 Gateway 设置都会移除。
          </p>
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

type ProviderListProps = {
  providers: Provider[];
  usageByProvider: Record<string, ProviderUsageSummary>;
  connectivityByProvider: Record<string, ProviderConnectivityReport>;
  busy: boolean;
  pendingKey: (key: string) => boolean;
  testingProviderId: string | null;
  onTest: (id: string) => Promise<void>;
  onDelete: (provider: Provider) => void;
  onViewModels: (providerId: string) => void;
  onRename: (providerId: string, name: string) => Promise<boolean>;
  onRequestAdd: () => void;
};

/**
 * @component ProviderList
 * @description 模型渠道列表：检测、查看模型目录和删除。
 */
export function ProviderList({
  providers,
  usageByProvider,
  connectivityByProvider,
  busy,
  pendingKey,
  testingProviderId,
  onTest,
  onDelete,
  onViewModels,
  onRename,
  onRequestAdd,
}: ProviderListProps) {
  const [renameTarget, setRenameTarget] = useState<Provider | null>(null);
  const [renameDraft, setRenameDraft] = useState("");
  const renaming = renameTarget
    ? (providers.find((provider) => provider.id === renameTarget.id) ??
      renameTarget)
    : null;
  const renamePending = renaming
    ? pendingKey(`provider:${renaming.id}`)
    : false;
  const renameName = renameDraft.trim();
  const renameInvalid = !renameName || renameName.length > 80;
  if (providers.length === 0)
    return (
      <div className="empty-state provider-empty">
        <strong>还没有模型渠道</strong>
        <span>
          添加 API Key 后，可以在这里检测连接、查看用量并将模型添加到主工作区。
        </span>
        <HintedButton
          type="button"
          title="添加第一个模型渠道"
          disabledReason="正在读取或保存状态，请稍候。"
          disabled={busy}
          onClick={onRequestAdd}
        >
          添加第一个模型渠道
        </HintedButton>
      </div>
    );

  return (
    <div className="provider-list">
      {providers.map((provider) => {
        const report = connectivityByProvider[provider.id];
        const usage = usageByProvider[provider.id];
        const isTesting = testingProviderId === provider.id;
        const resourcePending = pendingKey(`provider:${provider.id}`);
        const mutationReason =
          runningProviderLockReason(provider.lockedByInstances) ??
          (resourcePending ? "正在处理该模型渠道，请稍候。" : undefined);
        const testReason = !provider.configured
          ? "当前模型渠道没有完整配置 API Key，请先编辑并补全凭据。"
          : isTesting || resourcePending
            ? "正在处理该模型渠道，请稍候。"
            : undefined;

        return (
          <article className="provider-row" key={provider.id}>
            <div className="provider-main">
              <div className="provider-title">
                <StatusBadge provider={provider} report={report} />
                <strong>{provider.name}</strong>
              </div>
              <code title="上游 API 的访问根地址。">{provider.baseUrl}</code>
              <div className="provider-meta">
                <span title="API Key 只显示遮罩状态，原文保存在系统凭据库。">
                  {maskKey(provider.configured)}
                </span>
                <span>
                  {usageObservationLabel(provider.transportMode, usage)}
                </span>
                <span title="模型渠道的上游主机。">
                  {providerHost(provider.baseUrl)}
                </span>
                {usage?.lastRequestAt !== null &&
                  usage?.lastRequestAt !== undefined && (
                    <span title="最近一次经过 Gateway 的请求时间。">
                      最近活动 {formatTime(usage.lastRequestAt)}
                    </span>
                  )}
                {report && (
                  <>
                    <span
                      title={`最近一次连接检测于 ${formatTime(report.checkedAt)}。`}
                    >
                      检测于 {formatTime(report.checkedAt)}
                    </span>
                    <span>{protocolText(report?.wireApi)}</span>
                  </>
                )}
              </div>
              {usage && (
                <details className="provider-advanced">
                  <summary>余额与来源</summary>
                  <div
                    className="provider-stats"
                    aria-label="模型渠道用量与余额摘要"
                  >
                    <div>
                      <span>账户余额</span>
                      <strong>{balanceLabel(usage.upstreamBalance)}</strong>
                      <small title={balanceHint(usage.upstreamBalance)}>
                        {usage.upstreamBalance.currency ??
                          usage.upstreamBalance.unit}
                      </small>
                    </div>
                    <div>
                      <span>本地用量</span>
                      <strong>
                        {usageObservationLabel(provider.transportMode, usage)}
                      </strong>
                      <small>
                        {isDirectTransport(provider.transportMode)
                          ? "直连不经过 Gateway"
                          : "Gateway 观测"}
                      </small>
                    </div>
                    <div>
                      <span>今日 Token</span>
                      <strong>
                        {isDirectTransport(provider.transportMode) ||
                        usage.lastRequestAt === null
                          ? "—"
                          : tokenCount(tokenTotal(usage.today))}
                      </strong>
                      <small>输入 + 输出</small>
                    </div>
                  </div>
                </details>
              )}
            </div>
            <div
              className="provider-actions"
              aria-label={`${provider.name} 操作`}
            >
              <HintedButton
                className="icon-button provider-icon-action"
                type="button"
                size="md"
                aria-label="修改渠道名称"
                disabled={resourcePending}
                disabledReason="正在处理该模型渠道，请稍候。"
                onClick={() => {
                  setRenameTarget(provider);
                  setRenameDraft(provider.name);
                }}
              >
                <Pencil size={16} aria-hidden="true" />
              </HintedButton>
              <HintedButton
                className="icon-button provider-icon-action"
                type="button"
                size="md"
                aria-label="查看支持模型"
                disabled={isTesting}
                disabledReason="正在探测该模型渠道，请稍候。"
                onClick={() => onViewModels(provider.id)}
              >
                <Layers3 size={16} aria-hidden="true" />
              </HintedButton>
              <HintedButton
                className="icon-button provider-icon-action"
                type="button"
                size="md"
                aria-label={isTesting ? "正在更新渠道状态" : "更新渠道状态"}
                disabledReason={testReason}
                disabled={Boolean(testReason)}
                onClick={() => void onTest(provider.id)}
              >
                {isTesting ? (
                  <span className="loading-spinner" aria-hidden="true" />
                ) : (
                  <RefreshCw size={16} aria-hidden="true" />
                )}
              </HintedButton>
              <HintedButton
                className="icon-button provider-icon-action danger"
                type="button"
                size="md"
                title="删除模型渠道，并移除对应系统凭据和本地记录。"
                aria-label="删除模型渠道"
                disabledReason={mutationReason}
                disabled={Boolean(mutationReason)}
                onClick={() => onDelete(provider)}
              >
                <Trash2 size={16} aria-hidden="true" />
              </HintedButton>
            </div>
          </article>
        );
      })}
      {renaming ? (
        <ControlDialog
          onClose={() => {
            if (!renamePending) setRenameTarget(null);
          }}
          closeDisabled={renamePending}
          size={460}
          label={"修改 " + renaming.name}
          className="control-dialog-confirm"
        >
          <form
            className="launch-dialog"
            aria-labelledby="rename-provider-title"
            onSubmit={(event) => {
              event.preventDefault();
              if (renameInvalid || renamePending) return;
              if (renameName === renaming.name) {
                setRenameTarget(null);
                return;
              }
              void onRename(renaming.id, renameName).then((saved) => {
                if (saved) setRenameTarget(null);
              });
            }}
          >
            <div className="drawer-heading">
              <h2 id="rename-provider-title">修改 {renaming.name}</h2>
            </div>
            <label className="launch-directory-field">
              <span>渠道名称</span>
              <input
                value={renameDraft}
                maxLength={80}
                autoFocus
                aria-label="渠道名称"
                disabled={renamePending}
                onChange={(event) => setRenameDraft(event.target.value)}
              />
            </label>
            <p className="launch-dialog-copy">
              正在运行的实例仍使用启动时的名称，下次启动才换上新名称。
            </p>
            <div className="confirm-dialog-actions">
              <HintedButton
                className="secondary-action"
                type="button"
                disabled={renamePending}
                onClick={() => setRenameTarget(null)}
              >
                取消
              </HintedButton>
              <HintedButton
                className="primary-action"
                type="submit"
                disabled={renamePending || renameInvalid}
                disabledReason={
                  renamePending
                    ? "正在保存渠道名称。"
                    : "名称不能为空，最长 80 个字符。"
                }
              >
                {renamePending ? "正在保存…" : "保存"}
              </HintedButton>
            </div>
          </form>
        </ControlDialog>
      ) : null}
    </div>
  );
}

/**
 * @component ProviderOverlays
 * @description 模型渠道相关叠层：详情、模型目录、新增渠道和删除确认。
 */
export function ProviderOverlays() {
  const {
    usageByProvider,
    connectivityByProvider,
    showAddProvider,
    setShowAddProvider,
    setSelectedProviderId,
    setSelectedProviderModelCatalogId,
    deleteCandidate,
    setDeleteCandidate,
    selectedProvider,
    selectedProviderModelCatalog,
    pendingKey,
    testingProviderId,
    testProvider,
    createProvider,
    probeChannelDraft,
    confirmDeleteProvider,
  } = useAppState();
  return (
    <>
      {selectedProvider && (
        <div
          className="drawer-layer"
          role="presentation"
          onMouseDown={(event) => {
            if (event.target === event.currentTarget)
              setSelectedProviderId(null);
          }}
        >
          <aside
            className="detail-drawer"
            role="dialog"
            aria-modal="true"
            aria-label={`${selectedProvider.name} 详情`}
          >
            <div className="drawer-heading">
              <div>
                <p className="eyebrow">PROVIDER DETAIL</p>
                <h2>{selectedProvider.name}</h2>
              </div>
              <HintedButton
                className="icon-button circular-button"
                type="button"
                title="关闭详情"
                aria-label="关闭详情"
                onClick={() => setSelectedProviderId(null)}
              >
                <X size={17} aria-hidden="true" />
              </HintedButton>
            </div>
            <ConnectivityPanel
              provider={selectedProvider}
              report={connectivityByProvider[selectedProvider.id]}
            />
            <UsagePanel
              provider={selectedProvider}
              summary={usageByProvider[selectedProvider.id]}
            />
          </aside>
        </div>
      )}

      {selectedProviderModelCatalog && (
        <ProviderModelCatalogDrawer
          provider={selectedProviderModelCatalog}
          report={connectivityByProvider[selectedProviderModelCatalog.id]}
          onClose={() => setSelectedProviderModelCatalogId(null)}
          onRefresh={() => void testProvider(selectedProviderModelCatalog.id)}
          refreshing={testingProviderId === selectedProviderModelCatalog.id}
          refreshDisabledReason={
            !selectedProviderModelCatalog.configured
              ? "当前模型渠道没有完整配置 API Key，请先编辑并补全凭据。"
              : testingProviderId === selectedProviderModelCatalog.id ||
                  pendingKey(`provider:${selectedProviderModelCatalog.id}`)
                ? "正在处理该模型渠道，请稍候。"
                : undefined
          }
        />
      )}

      {showAddProvider && (
        <AddChannelDialog
          busy={pendingKey("provider:create")}
          onProbe={probeChannelDraft}
          onSubmit={createProvider}
          onClose={() => setShowAddProvider(false)}
        />
      )}

      {deleteCandidate && (
        <DeleteProviderDialog
          provider={deleteCandidate}
          busy={pendingKey(`provider:${deleteCandidate.id}`)}
          onCancel={() => setDeleteCandidate(null)}
          onConfirm={() => void confirmDeleteProvider()}
        />
      )}
    </>
  );
}

/** @see ProviderOverlays */
export const ChannelOverlays = ProviderOverlays;
