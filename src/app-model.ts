import { desktopChrome } from "./components/title-bar";
import {
  isDirectTransport,
  isGatewayTransport,
  type ApiKeyProviderSummary,
  type LaunchPresetSummary,
  type ManagedPresetModel,
  type OfficialAccountSummary,
  type RuntimeInstanceSummary,
  type SpecialModelRoleSummary,
  type TransportMode,
} from "./codex-control";

export type GatewayStatus = {
  state: "stopped" | "serving" | "unavailable";
  baseUrl: string | null;
  bindHost: string;
  port: number | null;
  activeBindings: number;
  inFlightRequests: number;
};
export type Provider = ApiKeyProviderSummary;
export type UsagePeriod = {
  requests: number;
  successes: number;
  inputTokens: number | null;
  outputTokens: number | null;
  estimatedCostUsd: number | null;
};
export type UpstreamBalance = {
  status: "supported" | "stale" | "unsupported" | "unauthorized" | "error";
  available: number | null;
  granted: number | null;
  used: number | null;
  unlimited: boolean;
  currency: string | null;
  unit: string;
  source: string;
  refreshedAt: number | null;
  expiresAt: number | null;
  error: string | null;
};
export type ProviderUsageSummary = {
  today: UsagePeriod;
  week: UsagePeriod;
  month: UsagePeriod;
  lastRequestAt: number | null;
  upstreamBalance: UpstreamBalance;
};
export type GatewayTimeoutSettings = {
  providerId: string;
  connectSeconds: number;
  firstByteSeconds: number;
  streamIdleSeconds: number;
  nonStreamTotalSeconds: number;
};
export type CheckStatus = "ready" | "failed" | "not_run";
export type ConnectivityStatus = "ready" | "degraded" | "unreachable" | "unauthorized";
export type DiscoveredModel = {
  upstreamModelId: string;
  displayName: string;
  ownedBy: string | null;
};
export type ProviderConnectivityReport = {
  providerId: string;
  checkedAt: number;
  status: ConnectivityStatus;
  model: string | null;
  models: DiscoveredModel[];
  modelsCheck: CheckStatus;
  textCheck: CheckStatus;
  errorCode: string | null;
  wireApi: "responses" | "chat_completions" | null;
};
export type GatewayDiagnosticEvent = {
  id: number;
  providerId: string;
  createdAt: number;
  source: "gateway" | "probe";
  route: string;
  model: string | null;
  transportMode: TransportMode;
  stream: boolean;
  outcome: "success" | "failed";
  httpStatus: number | null;
  latencyMs: number | null;
  errorCode: string | null;
};
export type InstanceRequestDiagnostic = {
  id: number;
  instanceId: string | null;
  sessionId: string | null;
  createdAt: number;
  route: string;
  stream: boolean;
  wireApi: string;
  publicModelId: string;
  upstreamModelId: string;
  providerId: string;
  providerRevisionId: string;
  httpStatus: number | null;
  responseContentType: string | null;
  firstByteMs: number | null;
  totalLatencyMs: number | null;
  outcome: "success" | "timeout" | "interrupted" | "format_error" | "failed";
  errorCode: string | null;
  errorDetail: string | null;
  inputLength: number;
  outputLength: number;
  inputTokens: number | null;
  outputTokens: number | null;
  totalTokens: number | null;
  inputHash: string | null;
  outputHash: string | null;
  toolCallTypes: string[];
  toolResultHashes: string[];
};
export type ConversationSummary = {
  id: number;
  instanceId: string;
  sessionId: string | null;
  createdAt: number;
  inputLength: number;
  outputLength: number;
  inputHash: string | null;
  outputHash: string | null;
  toolCallTypes: string[];
  toolResultHashes: string[];
};
export type NavView = "credentials" | "presets" | "instances" | "diagnostics";
export type UtilityDrawerView = Exclude<NavView, "presets">;
export type ThemeMode = "light" | "dark" | "system";
export const accentNames = ["cyan", "teal", "blue", "violet", "orange"] as const;
export type AccentName = (typeof accentNames)[number];
export const defaultAccentColor = "#1098ad";
export type AuthorizationMode = "browser" | "device_code";
export type AuthorizationDraft = {
  accountId?: string;
  workspaceLabel: string;
  mode: AuthorizationMode;
};
export type ActiveAuthorization = {
  attemptId: string;
  accountId: string;
  prompt: { type: "browser" } | { type: "device_code"; userCode: string };
};
export type PresetModelDraft = {
  sourceKey: string;
  upstreamModelId: string;
  publicModelId: string;
  displayName: string;
  contextWindow?: number;
  autoCompactTokenLimit?: number;
  reasoningLevels?: string[];
  defaultReasoningLevel?: string;
};

export type DiagnosticEventModel = {
  id: number;
  providerId: string;
  createdAt: number;
  source: "gateway" | "probe";
  route: string;
  model: string | null;
  stream: boolean;
  outcome: "success" | "failed";
  httpStatus: number | null;
  latencyMs: number | null;
  errorCode: string | null;
};

export type RequestEventModel = {
  id: number;
  providerId: string;
  publicModelId: string;
  upstreamModelId: string;
  createdAt: number;
  instanceId: string | null;
  wireApi: string;
  stream: boolean;
  outcome: string;
  httpStatus: number | null;
  errorCode: string | null;
  errorDetail: string | null;
  totalLatencyMs: number | null;
};

export function reasoningLevelsForModel(modelId: string): string[] {
  const model = modelId.toLowerCase();
  if (model.includes("grok")) {
    if (model.includes("4.3") || model.includes("4-3"))
      return ["none", "low", "medium", "high", "xhigh"];
    if (model.includes("4.5") || model.includes("4-5"))
      return ["low", "medium", "high"];
    return ["low", "medium", "high", "xhigh"];
  }
  if (model.includes("deepseek")) return ["high"];
  if (model.includes("gpt-") || model.includes("openai"))
    return ["low", "medium", "high", "xhigh", "max", "ultra"];
  return ["low", "medium", "high", "xhigh"];
}

export function defaultReasoningLevel(levels: string[]): string | undefined {
  return levels.includes("high") ? "high" : levels[0];
}

export function optionalWindowValue(
  value: number | null | undefined,
): number | undefined {
  return typeof value === "number" && Number.isInteger(value)
    ? value
    : undefined;
}

export function windowInputValue(value: number | undefined): string {
  return value === undefined ? "" : String(value);
}

export function parseOptionalWindowInput(raw: string): number | undefined {
  const trimmed = raw.trim();
  if (!trimmed) return undefined;
  if (!/^\d+$/.test(trimmed)) return undefined;
  const parsed = Number(trimmed);
  return Number.isSafeInteger(parsed) ? parsed : undefined;
}

export function modelSelectionPayload(model: {
  sourceKind: ManagedPresetModel["sourceKind"];
  sourceId: string;
  upstreamModelId: string;
  publicModelId: string;
  displayName: string;
  contextWindow?: number;
  autoCompactTokenLimit?: number;
  reasoningLevels?: string[];
  defaultReasoningLevel?: string;
}) {
  return {
    sourceKind: model.sourceKind,
    sourceId: model.sourceId,
    upstreamModelId: model.upstreamModelId,
    publicModelId: model.publicModelId,
    displayName: model.displayName,
    contextWindow: model.contextWindow,
    autoCompactTokenLimit: model.autoCompactTokenLimit,
    reasoningLevels: model.reasoningLevels,
    defaultReasoningLevel: model.defaultReasoningLevel ?? undefined,
  };
}

export function defaultPublicModelId(modelId: string) {
  const normalized = modelId
    .toLowerCase()
    .replace(/[^a-z0-9._-]/g, "-")
    .replace(/-+/g, "-")
    .replace(/^-|-$/g, "");
  return normalized || "model";
}


export function specialRoleStatusLabel(role: SpecialModelRoleSummary) {
  if (role.status === "native_available") return "已提供原生能力（隐藏）";
  if (role.status === "replacement_available") return "可配置替代模型";
  return "当前版本未提供可替代合同";
}

export function providerModelSlug(providerId: string) {
  const normalized = defaultPublicModelId(providerId);
  return normalized === "model"
    ? `provider-${providerId.slice(0, 8)}`
    : normalized.slice(0, 32);
}

export function modelDraftKey(sourceKey: string, upstreamModelId: string) {
  return `${sourceKey}\u0000${upstreamModelId}`;
}

export const stoppedGateway: GatewayStatus = {
  state: "stopped",
  baseUrl: null,
  bindHost: "127.0.0.1",
  port: null,
  activeBindings: 0,
  inFlightRequests: 0,
};
export const defaultGatewaySettings = (providerId = ""): GatewayTimeoutSettings => ({
  providerId,
  connectSeconds: 10,
  firstByteSeconds: 30,
  streamIdleSeconds: 45,
  nonStreamTotalSeconds: 120,
});
export const themeStorageKey = "rcodex.theme";
export const accentStorageKey = "rcodex-accent";
// Official-account OAuth is retained in the data model for a future release,
// but is intentionally unavailable in the current product surface.
export const OFFICIAL_AUTH_ENABLED = false;
export const transportLabels: Record<TransportMode, string> = {
  direct_responses: "直连 Responses",
  gateway_passthrough: "Gateway 透传",
  gateway_chat_completions: "Gateway 转换 Chat（文本）",
};
export const gatewayHint =
  "Gateway 是本地路由服务，负责请求透传、协议转换、统一诊断和超时控制。直连 Responses 时不需要启动它。";
export function gatewayStateLabel(
  state: GatewayStatus["state"],
) {
  if (state === "serving") return "Gateway 运行中";
  if (state === "unavailable") return "Gateway 不可用";
  return "Gateway 已停止";
}


if (typeof document !== "undefined") {
  document.documentElement.dataset.chrome = desktopChrome();
}
export const navHints: Record<NavView, string> = {
  credentials:
    "管理 API Key 模型渠道；这里的修改不会自动加入主工作区或改变任何 Codex。官方账户认证暂时关闭。",
  presets: "配置 R Codex 工作区使用的模型渠道，然后启动 Codex App 或 CLI。",
  instances:
    "查看由 R Codex 启动的 Codex CLI 或受管理 App 实例及其固定方案，并显式停止实例。",
  diagnostics:
    "查看脱敏后的连接结果、HTTP 状态、延迟和错误码，不包含请求正文或 API Key。",
};

export function modeDescription(mode: TransportMode) {
  if (isDirectTransport(mode))
    return "Codex 直接访问上游，不启动本地 Gateway。";
  if (mode === "gateway_passthrough")
    return "经本地 Gateway 原样转发 Responses 与 SSE。";
  return "经本地 Gateway 将文本 Responses 转为 Chat Completions。";
}
export function isGatewayPreset(preset: LaunchPresetSummary) {
  return (
    preset.managedRouting === true || isGatewayTransport(preset.transportMode)
  );
}
export function routePathLabel(preset: LaunchPresetSummary) {
  return isGatewayPreset(preset)
    ? "Codex CLI → 本机 Gateway → Provider Base URL"
    : "Codex CLI → Provider Base URL";
}
export function diagnosticLayer(
  errorCode: string | null,
  source: GatewayDiagnosticEvent["source"],
): { label: string; className: string; hint: string } {
  if (source === "probe")
    return {
      label: "模型渠道检测",
      className: "probe",
      hint: "这是主动检测 Provider 的结果，不代表受管理 CLI 已发出请求。",
    };
  if (!errorCode)
    return {
      label: "Gateway 请求",
      className: "gateway",
      hint: "请求已进入本机 Gateway。",
    };
  if (
    errorCode.startsWith("upstream_") ||
    errorCode === "invalid_upstream_response"
  )
    return {
      label: "上游服务",
      className: "upstream",
      hint: "请求已经经过本机 Gateway，但真实 Provider 返回失败或不兼容响应。",
    };
  if (
    errorCode.includes("instance") ||
    errorCode.includes("route") ||
    errorCode.includes("model")
  )
    return {
      label: "Gateway / 路由",
      className: "gateway",
      hint: "请求到达本机 Gateway，但实例准入或模型路由没有通过。",
    };
  return {
    label: "Gateway 请求",
    className: "gateway",
    hint: "请求已进入本机 Gateway，详见错误码。",
  };
}
export function instanceDiagnosticHint(
  instance: RuntimeInstanceSummary,
  preset?: LaunchPresetSummary,
) {
  if (instance.state === "exited")
    return instance.exitCode === null || instance.exitCode === undefined
      ? "进程已退出，退出码不可用（例如 Terminal 被直接关闭）。"
      : `进程已退出，退出码 ${instance.exitCode}。`;
  if (instance.state === "failed")
    return "启动器未能完成本次 Codex 启动，请展开启动日志查看具体阶段。";
  if (instance.state === "orphaned")
    return "启动回执未能确认进程身份；重新核验或停止实例后才会释放配置锁。";
  if (instance.state === "stopping")
    return "已发出停止请求；若进程尚未退出，可重新核验后再次停止。";
  if (instance.state === "degraded")
    return "Gateway 曾中断，本实例仍需核验；请求可能暂时不可用。";
  return preset
    ? `${routePathLabel(preset)}；实例状态由启动回执和进程身份自动核验。`
    : "实例状态由启动回执和进程身份自动核验。";
}
export function diagnosticNextStep(event: GatewayDiagnosticEvent) {
  if (event.source === "probe")
    return event.outcome === "success"
      ? "可将该渠道加入启动方案。"
      : "检查 Base URL、API Key 和上游模型协议。";
  if (event.outcome === "success")
    return "请求已完成，可继续观察延迟和模型路由。";
  if (!event.errorCode) return "查看实例启动日志，确认 Gateway 是否仍在监听。";
  if (
    event.errorCode.startsWith("upstream_") ||
    event.errorCode === "invalid_upstream_response"
  )
    return "检查真实 Provider 的可用性、模型名和协议响应。";
  if (
    event.errorCode.includes("instance") ||
    event.errorCode.includes("route") ||
    event.errorCode.includes("model")
  )
    return "检查启动方案中的模型映射，并重新启动实例。";
  return "查看错误码对应的 Gateway 日志并重试。";
}
export function tokenCount(value: number | null) {
  return value === null ? "—" : value.toLocaleString();
}
export function tokenTotal(period: UsagePeriod) {
  return period.inputTokens === null || period.outputTokens === null
    ? null
    : period.inputTokens + period.outputTokens;
}
export function successRate(period: UsagePeriod) {
  return period.requests === 0
    ? "—"
    : `${Math.round((period.successes / period.requests) * 100)}%`;
}
export function balanceAmount(balance: UpstreamBalance) {
  return balance.available === null
    ? null
    : balance.available.toLocaleString(undefined, { maximumFractionDigits: 2 });
}
export function balanceLabel(balance: UpstreamBalance) {
  if (balance.unlimited) return "不限额";
  const amount = balanceAmount(balance);
  if (balance.status === "supported")
    return amount === null
      ? "已支持"
      : `${balance.currency ? `${balance.currency} ` : ""}${amount}`;
  if (balance.status === "stale")
    return amount === null
      ? "缓存"
      : `${balance.currency ? `${balance.currency} ` : ""}${amount}（缓存）`;
  if (balance.status === "unauthorized") return "认证失败";
  if (balance.status === "error") return "查询失败";
  return "暂不支持";
}
export function balanceHint(balance: UpstreamBalance) {
  const status =
    balance.status === "supported"
      ? "最近一次查询成功"
      : balance.status === "stale"
        ? "上游暂时不可用，显示最近一次成功缓存"
        : balance.status === "unauthorized"
          ? "上游拒绝了 API Key"
          : balance.status === "error"
            ? "余额查询失败"
            : "该模型渠道没有已配置的余额接口";
  const refreshed =
    balance.refreshedAt === null
      ? "尚未成功刷新"
      : `刷新于 ${formatTime(balance.refreshedAt)}`;
  return `${status}；${refreshed}；来源：${balance.source}${balance.error ? `；错误码：${balance.error}` : ""}`;
}
export function checkLabel(status: CheckStatus) {
  return status === "ready" ? "通过" : status === "failed" ? "失败" : "未执行";
}
export function formatTime(timestamp: number) {
  return new Date(timestamp * 1000).toLocaleString();
}
export function shortId(value?: string | null) {
  return value
    ? value.length > 18
      ? `${value.slice(0, 9)}…${value.slice(-6)}`
      : value
    : "—";
}
export function quotaWindowLabel(minutes?: number | null) {
  if (!minutes) return "窗口";
  if (minutes % 10080 === 0) return `${minutes / 10080} 周`;
  if (minutes % 1440 === 0) return `${minutes / 1440} 天`;
  if (minutes % 60 === 0) return `${minutes / 60} 小时`;
  return `${minutes} 分钟`;
}
export function officialQuotaLabel(account: OfficialAccountSummary) {
  if (!account.quota) return "当前版本暂未获取官方限额";
  const values = [
    account.quota.primaryUsedPercent,
    account.quota.secondaryUsedPercent,
  ].filter((value): value is number => typeof value === "number");
  if (values.length === 0)
    return account.quota.errorCode ? "官方限额读取失败" : "官方未返回限额窗口";
  return `已用 ${values.map((value) => `${Math.round(value)}%`).join(" / ")}${account.quota.expiresAt <= Math.floor(Date.now() / 1000) ? "（缓存）" : ""}`;
}
export function maskKey(configured: boolean) {
  return configured ? "••••••••••••" : "未配置";
}
export function getInitialThemeMode(): ThemeMode {
  if (typeof window === "undefined") return "system";
  try {
    const stored = window.localStorage?.getItem(themeStorageKey);
    return stored === "light" || stored === "dark" || stored === "system"
      ? stored
      : "system";
  } catch {
    return "system";
  }
}
const namedAccentHex: Record<AccentName, string> = {
  cyan: "#1098ad",
  teal: "#0ca678",
  blue: "#1c7ed6",
  violet: "#7048e8",
  orange: "#f76707",
};
export function normalizeAccentColor(value: string | null | undefined) {
  if (!value) return defaultAccentColor;
  if (value in namedAccentHex) return namedAccentHex[value as AccentName];
  const hex = value.startsWith("#") ? value : "#" + value;
  return /^#[0-9a-fA-F]{6}$/.test(hex) ? hex.toLowerCase() : defaultAccentColor;
}
export function getInitialAccent() {
  if (typeof window === "undefined") return defaultAccentColor;
  try {
    return normalizeAccentColor(window.localStorage?.getItem(accentStorageKey));
  } catch {
    return defaultAccentColor;
  }
}
export function prefersDarkTheme() {
  return (
    typeof window !== "undefined" &&
    window.matchMedia("(prefers-color-scheme: dark)").matches
  );
}
