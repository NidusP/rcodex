import { invoke } from "@tauri-apps/api/core";

export type TransportMode = "direct_responses" | "gateway_passthrough" | "gateway_chat_completions";
export type CodexAuthSummary = { state: "configured" | "not_detected" | "unreadable" | string; method?: string | null; sourcePath?: string | null; updatedAt?: number | null };
export type CodexInstallationStatus = { installed: boolean; executablePath?: string | null; version?: string | null; executableUpdatedAt?: number | null; appPath?: string | null; appVersion?: string | null; appUpdatedAt?: number | null; auth: CodexAuthSummary; error?: string | null };
export type CodexUpdateStatus = { state: "up_to_date" | "update_available" | "unavailable" | string; currentVersion?: string | null; latestVersion?: string | null; publishedAt?: string | null; releaseUrl?: string | null; notes?: string | null; checkedAt: number; error?: string | null };
export type CredentialKind = "api_key" | "official" | "none";
export type InstanceState = "launching" | "running" | "stopping" | "orphaned" | "degraded" | "exited" | "failed";
export type OfficialAuthState = "authorizing" | "awaiting_user" | "verifying" | "authorized" | "needs_reauth" | "failed" | "protocol_incompatible" | "disconnected";
export type AuthorizationAttemptState = "authorizing" | "awaiting_user" | "verifying" | "authorized" | "cancelled" | "failed" | "interrupted";

export type AuthorizationStarted = {
  attemptId: string;
  accountId: string;
  credentialRevisionId: string;
  state: AuthorizationAttemptState;
  prompt: { type: "browser"; authUrl: string } | { type: "device_code"; verificationUrl: string; userCode: string };
};

export type AuthorizationStatus = {
  attemptId: string;
  accountId?: string | null;
  credentialRevisionId?: string | null;
  state: AuthorizationAttemptState;
  errorCode?: string | null;
};

export type LockedInstanceRef = {
  instanceId: string;
  state: InstanceState;
};

export type ApiKeyProviderSummary = {
  id: string;
  name: string;
  baseUrl: string;
  isDefault: boolean;
  configured: boolean;
  transportMode: TransportMode;
  currentRevisionId?: string | null;
  lockedByInstances: LockedInstanceRef[];
};

export type OfficialQuotaSummary = {
  primaryUsedPercent?: number | null;
  primaryWindowMinutes?: number | null;
  primaryResetsAt?: number | null;
  secondaryUsedPercent?: number | null;
  secondaryWindowMinutes?: number | null;
  secondaryResetsAt?: number | null;
  refreshedAt: number;
  expiresAt: number;
  errorCode?: string | null;
};

export type OfficialAccountSummary = {
  id: string;
  maskedIdentity: string;
  workspaceLabel?: string | null;
  planLabel?: string | null;
  authState: OfficialAuthState;
  authCheckedAt?: number | null;
  authErrorCode?: string | null;
  codexVersion?: string | null;
  schemaFingerprint?: string | null;
  quota?: OfficialQuotaSummary | null;
  currentCredentialRevisionId?: string | null;
  lockedByInstances: LockedInstanceRef[];
};

export type LaunchPresetSummary = {
  id: string;
  entryKind?: "managed_app" | string | null;
  name: string;
  version: number;
  credentialKind: CredentialKind;
  credentialId: string;
  credentialLabel: string;
  workspacePath?: string | null;
  transportMode?: TransportMode | null;
  isPreferred?: boolean;
  lockedByInstances: LockedInstanceRef[];
  compatibilityFallback?: boolean;
  managedRouting?: boolean;
  compositionState?: string | null;
  officialAccountId?: string | null;
  providerIds: string[];
  loginProviderId?: string | null;
  models: ManagedPresetModel[];
  autoReviewPublicModelId?: string | null;
};

export type ManagedPresetModel = {
  routeIdentityId: string;
  sourceKind: "official" | "api_provider";
  sourceId: string;
  upstreamModelId: string;
  publicModelId: string;
  displayName: string;
  contextWindow?: number | null;
  autoCompactTokenLimit?: number | null;
  reasoningLevels?: string[];
  defaultReasoningLevel?: string | null;
};

export type SpecialModelRoleSummary = {
  roleKey: string;
  displayName: string;
  invocationSummary: string;
  nativeModelId?: string | null;
  nativeAvailable: boolean;
  overrideable: boolean;
  requiredWireApi?: string | null;
  status: "native_available" | "replacement_available" | "unavailable" | string;
};

type RawManagedComposition = {
  launchPresetId: string;
  officialAccountId?: string | null;
  apiKeyProviderIds?: string[];
  models?: ManagedPresetModel[];
  autoReviewPublicModelId?: string | null;
  state: string;
};

export type RuntimeInstanceSummary = {
  id: string;
  launchPresetId?: string | null;
  launchConfigurationRevisionId: string;
  launchTarget?: string | null;
  credentialKind: CredentialKind;
  credentialLabel: string;
  credentialRevisionId?: string | null;
  workspacePath?: string | null;
  state: InstanceState;
  supervisorPid?: number | null;
  pid?: number | null;
  launchedAt?: number | null;
  exitedAt?: number | null;
  exitCode?: number | null;
  startupLog?: string | null;
};

export type CodexCompatibility = {
  controlPlaneAvailable: boolean;
  officialAccountsAvailable: boolean;
  /** False until the desktop renderer release probe passes. */
  managedAppAvailable: boolean;
  /** False until the official credential proxy contract is verified. */
  officialHybridAvailable: boolean;
  code?: string | null;
  message?: string | null;
};

export type CodexControlSnapshot = {
  accounts: OfficialAccountSummary[];
  providers: ApiKeyProviderSummary[];
  presets: LaunchPresetSummary[];
  instances: RuntimeInstanceSummary[];
  compatibility: CodexCompatibility;
};

export type CommandError = {
  code: string;
  message: string;
  conflictingInstances: LockedInstanceRef[];
};

type RawAccount = Partial<OfficialAccountSummary> & { id: string };
type RawProvider = Omit<ApiKeyProviderSummary, "lockedByInstances"> & { lockedByInstances?: unknown };
type RawPreset = Partial<LaunchPresetSummary> & {
  id: string;
  name: string;
  credentialKind: string;
  apiKeyProviderId?: string | null;
  codexAccountId?: string | null;
  gatewayPolicy?: string | null;
  defaultWorkspacePath?: string | null;
};
type RawInstance = Partial<RuntimeInstanceSummary> & {
  id: string;
  launchConfigurationRevisionId: string;
  credentialKind: string;
};
type RawSnapshot = {
  accounts?: RawAccount[];
  providers?: RawProvider[];
  presets?: RawPreset[];
  instances?: RawInstance[];
  compatibility?: CodexCompatibility;
};

const activeStates = new Set<InstanceState>(["launching", "running", "stopping", "orphaned", "degraded"]);
const transportModes = new Set<TransportMode>(["direct_responses", "gateway_passthrough", "gateway_chat_completions"]);

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function normalizeInstanceState(value: unknown): InstanceState {
  return value === "launching" || value === "running" || value === "stopping" || value === "orphaned" || value === "degraded" || value === "exited" || value === "failed" ? value : "orphaned";
}

function normalizeCredentialKind(value: unknown): CredentialKind {
  if (value === "none") return "none";
  return value === "official" ? "official" : "api_key";
}

function normalizeAuthState(value: unknown): OfficialAuthState {
  if (value === "active") return "authorized";
  return value === "authorizing" || value === "awaiting_user" || value === "verifying" || value === "authorized" || value === "needs_reauth" || value === "failed" || value === "protocol_incompatible" || value === "disconnected" ? value : "disconnected";
}

function normalizeTransportMode(value: unknown): TransportMode | null {
  if (value === "direct") return "direct_responses";
  return typeof value === "string" && transportModes.has(value as TransportMode) ? value as TransportMode : null;
}

export function normalizeLocks(value: unknown): LockedInstanceRef[] {
  if (!Array.isArray(value)) return [];
  return value.flatMap((entry) => {
    if (typeof entry === "string") return [{ instanceId: entry, state: "running" as const }];
    if (!isRecord(entry)) return [];
    const instanceId = typeof entry.instanceId === "string" ? entry.instanceId : typeof entry.id === "string" ? entry.id : null;
    return instanceId ? [{ instanceId, state: normalizeInstanceState(entry.state) }] : [];
  });
}

function uniqueLocks(groups: LockedInstanceRef[][]): LockedInstanceRef[] {
  const locks = new Map<string, LockedInstanceRef>();
  for (const group of groups) for (const lock of group) locks.set(lock.instanceId, lock);
  return [...locks.values()];
}

function normalizeSnapshot(snapshot: RawSnapshot, listedProviders: RawProvider[], compositions: RawManagedComposition[] = []): CodexControlSnapshot {
  const accounts = (snapshot.accounts ?? []).map<OfficialAccountSummary>((account) => ({
    ...account,
    maskedIdentity: account.maskedIdentity?.trim() || "未识别账户",
    authState: normalizeAuthState(account.authState),
    lockedByInstances: normalizeLocks(account.lockedByInstances),
  }));
  const baseProviders = snapshot.providers?.length ? snapshot.providers : listedProviders;
  const providerById = new Map(baseProviders.map((provider) => [provider.id, provider]));
  const accountById = new Map(accounts.map((account) => [account.id, account]));
  const compositionByPreset = new Map(compositions.map((composition) => [composition.launchPresetId, composition]));
  const presets = (snapshot.presets ?? []).map<LaunchPresetSummary>((preset) => {
    const credentialKind = normalizeCredentialKind(preset.credentialKind);
    const credentialId = preset.credentialId ?? (isOfficialCredential(credentialKind) ? preset.codexAccountId : isApiKeyCredential(credentialKind) ? preset.apiKeyProviderId : null) ?? "";
    const provider = providerById.get(credentialId);
    const account = accountById.get(credentialId);
    const composition = compositionByPreset.get(preset.id);
    const providerIds = composition?.apiKeyProviderIds ?? (isApiKeyCredential(credentialKind) && credentialId ? [credentialId] : []);
    const officialAccountId = composition?.officialAccountId ?? (isOfficialCredential(credentialKind) ? credentialId : null);
    const managedRouting = composition?.state === "ready" && Boolean(composition.models?.length);
    const sourceLabels = [
      officialAccountId ? accountById.get(officialAccountId)?.maskedIdentity ?? "官方账户" : null,
      ...providerIds.map((id) => providerById.get(id)?.name ?? "模型渠道"),
    ].filter((value): value is string => Boolean(value));
    const configuredTransportMode = normalizeTransportMode(preset.gatewayPolicy);
    const hasAutomaticReviewRoute = composition?.models?.some((model) =>
      model.sourceKind === "api_provider" && providerById.get(model.sourceId)?.transportMode !== "gateway_chat_completions"
    ) ?? false;
    const managedTransportMode: TransportMode | null = managedRouting
      ? hasAutomaticReviewRoute
        ? "gateway_passthrough"
        : configuredTransportMode ?? "gateway_passthrough"
      : null;
    return {
      id: preset.id,
      entryKind: preset.entryKind ?? null,
      name: preset.name,
      version: Number.isFinite(preset.version) ? preset.version as number : 0,
      credentialKind,
      credentialId,
      credentialLabel: managedRouting ? sourceLabels.join(" + ") : preset.credentialLabel ?? (isOfficialCredential(credentialKind) ? account?.maskedIdentity : isApiKeyCredential(credentialKind) ? provider?.name : "未配置模型渠道") ?? "凭据不可用",
      workspacePath: preset.workspacePath ?? preset.defaultWorkspacePath,
      transportMode: managedTransportMode ?? preset.transportMode ?? normalizeTransportMode(preset.gatewayPolicy) ?? provider?.transportMode ?? null,
      isPreferred: preset.isPreferred ?? false,
      lockedByInstances: normalizeLocks(preset.lockedByInstances),
      compatibilityFallback: preset.compatibilityFallback,
      managedRouting,
      compositionState: composition?.state,
      officialAccountId,
      providerIds,
      loginProviderId: preset.loginProviderId ?? (providerIds.length === 1 ? providerIds[0] : null),
      models: composition?.models ?? [],
      autoReviewPublicModelId: composition?.autoReviewPublicModelId ?? null,
    };
  });
  const presetById = new Map(presets.map((preset) => [preset.id, preset]));
  const instances = (snapshot.instances ?? []).map<RuntimeInstanceSummary>((instance) => {
    const credentialKind = normalizeCredentialKind(instance.credentialKind);
    const preset = instance.launchPresetId ? presetById.get(instance.launchPresetId) : undefined;
    return {
      id: instance.id,
      launchPresetId: instance.launchPresetId,
      launchConfigurationRevisionId: instance.launchConfigurationRevisionId,
      launchTarget: instance.launchTarget,
      credentialKind,
      credentialLabel: instance.credentialLabel ?? preset?.credentialLabel ?? (isOfficialCredential(credentialKind) ? "官方账户" : "模型渠道"),
      credentialRevisionId: instance.credentialRevisionId,
      workspacePath: instance.workspacePath,
      state: normalizeInstanceState(instance.state),
      supervisorPid: instance.supervisorPid,
      pid: instance.pid,
      launchedAt: instance.launchedAt,
      exitedAt: instance.exitedAt,
      exitCode: instance.exitCode,
      startupLog: typeof instance.startupLog === "string" ? instance.startupLog : null,
    };
  });
  const providers = baseProviders.map<ApiKeyProviderSummary>((provider) => {
    const presetLocks = presets.filter((preset) => isApiKeyCredential(preset.credentialKind) && preset.credentialId === provider.id).map((preset) => preset.lockedByInstances);
    return { ...provider, lockedByInstances: uniqueLocks([normalizeLocks(provider.lockedByInstances), ...presetLocks]) };
  });
  const compatibility = snapshot.compatibility
    ? {
        ...snapshot.compatibility,
        managedAppAvailable: snapshot.compatibility.managedAppAvailable ?? false,
        officialHybridAvailable: snapshot.compatibility.officialHybridAvailable ?? false,
      }
    : {
        controlPlaneAvailable: true,
        officialAccountsAvailable: false,
        managedAppAvailable: false,
        officialHybridAvailable: false,
        code: "official_auth_unavailable",
        message: "当前后端已启用 Codex 启动方案和运行实例管理，官方账户授权适配器尚未启用。API Key 模型渠道可正常使用。",
      };
  return {
    accounts,
    providers,
    presets,
    instances,
    compatibility,
  };
}

const legacyErrorMessages: Record<string, string> = {
  login_provider_required: "请选择一个工作区渠道作为 API 登录。",
  managed_gateway_legacy_retirement_failed: "旧版本机网关尚未完成升级，请退出旧 R Codex 网关后重试；未操作原 Codex 配置。",
  login_provider_not_in_workspace: "登录渠道已移出工作区，请重新选择。",
  isolated_auth_key_invalid: "登录渠道的密钥不可用，请更新密钥后重试。",
  managed_path_link_rejected: "R Codex 环境目录被替换为链接，已阻止启动以保护原 Codex。",
  managed_path_outside_owned_root: "R Codex 环境目录超出应用管理范围，已阻止操作。",
  configuration_in_use: "这份配置正在被运行中的隔离实例使用，停止该实例后才能修改。",
  resource_operation_in_progress: "资源正在执行另一项配置操作，请稍后重试。",
  version_conflict: "Codex 启动方案已被更新，请刷新后再试。",
  launch_preset_not_found: "Codex 启动方案不存在或已被删除。",
  official_launch_requires_authorized_adapter: "当前版本尚未启用官方账户启动适配器。",
  official_auth_disabled: "官方账户认证暂时关闭；当前版本仅支持 API Key 模型渠道。",
  official_credential_in_use: "该官方账户已有活动 Codex CLI，请先停止现有实例。",
  official_account_needs_reauth: "官方账户认证已失效，请重新授权后再启动。",
  official_auth_region_unsupported: "当前网络区域不支持官方 Codex 授权，请切换到受支持的网络后重试。",
  managed_app_unavailable: "当前 Codex 版本/平台尚未通过受管理 App 兼容性探针。",
  managed_app_requires_gateway: "受管理 Codex App 当前只支持经过本机 Gateway 的启动方案。",
  managed_app_requires_managed_preset: "受管理 App 必须使用已准备好的多 Provider 启动方案。",
  managed_app_in_use: "这个隔离实例的 Codex App 正在运行。其他实例可以同时启动；请先停止这一条，再重新启动或同步它的设置。",
  managed_computer_use_client_missing: "隔离环境缺少 Computer Use 客户端。请确认已安装 Codex Desktop 后重试；不会使用默认 Codex 目录中的客户端。",
  managed_computer_use_node_missing: "未找到 Codex Desktop 内的 Computer Use Node 运行时，已阻止启动。",
  managed_computer_use_node_repl_missing: "未找到 Codex Desktop 内的 Browser 本地服务，已阻止启动。",
  managed_computer_use_cua_repl_missing: "未找到 Codex Desktop 内的 Computer Use 本地服务，已阻止启动。",
  managed_computer_use_codex_cli_missing: "未找到 Codex Desktop 内嵌 CLI，已阻止启动隔离 Computer Use。",
  managed_computer_use_install_failed: "无法把 Computer Use 客户端安装到隔离目录，已阻止启动。",
  managed_computer_use_default_home_rejected: "已阻止从默认 Codex 目录安装 Computer Use 客户端。",
  isolated_home_not_ready: "还没有隔离 CODEX_HOME。请先启动一次隔离 App。",
  invalid_launch_target: "Codex 启动目标无效。",
  authorization_browser_open_failed: "无法打开官方授权页面，本次授权已取消。",
  authorization_browser_open_unsupported: "当前系统尚不支持自动打开官方授权页面。",
  credential_missing: "启动凭据缺失或已失效。",
  launch_unconfirmed: "Codex 启动结果未能确认，请到运行实例中核验。",
  invalid_gateway_policy: "Codex 启动方案的 Gateway 策略无效。",
  invalid_launch_preset: "Codex 启动方案的凭据组合无效。",
  invalid_launch_preset_name: "Codex 启动方案名称无效。",
  route_set_has_no_models: "请至少添加一个模型路由。",
  duplicate_model_selection: "模型路由存在重复的公共 ID 或上游来源。",
  reserved_internal_model: "Codex 内部审核/编排模型不能作为普通模型加入启动方案。",
  invalid_model_window: "上下文窗口和自动压缩阈值必须是 1 到 999999 的整数，且压缩阈值不能大于窗口。空值表示使用族默认。禁止填写 1000000。",
  auto_review_model_not_selected: "自动审核模型必须来自当前启动方案已经选择的普通模型。",
  auto_review_model_requires_api_provider: "自动审核模型当前只支持 API Key 模型渠道。",
  auto_review_model_requires_responses: "自动审核模型必须来自 Responses 协议的模型渠道；Chat 文本转换无法保证审核工具能力。",
  duplicate_provider: "同一个启动方案不能重复选择模型渠道。",
  route_source_not_selected: "模型来源不属于当前选择的官方账户或模型渠道。",
  invalid_public_model_id: "公共模型 ID 只能使用小写字母、数字、点、下划线和连字符，且不能包含斜杠。",
  model_public_id_immutable: "该上游模型已绑定稳定公共 ID；请沿用原公共 ID。",
  launch_preset_needs_model_selection: "Codex 启动方案尚未选择模型，不能启动。",
  managed_profile_options_invalid: "工作区设置无法读取，请刷新后再试。",
  official_hybrid_route_experimental_disabled: "官方订阅模型经自定义 Gateway 的公开协议尚未验证；混合启动当前保持实验关闭。",
  managed_gateway_not_serving: "R Codex 本地路由服务未能启动。",
  managed_gateway_sidecar_start_timeout: "R Codex 本地路由服务启动超时。若隔离 CLI 或 App 仍在运行，现有网关可能仍在转发，请先停止实例后再启动。",
  managed_gateway_port_in_use: "本机回环路由端口已被占用，无法启动新的 Gateway。隔离实例可能仍在使用现有网关；请先停止实例，或关闭占用该端口的进程后重试。",
  managed_gateway_cleanup_process_not_found: "无法识别占用端口的 R Codex Gateway，未执行清理。",
  managed_gateway_cleanup_foreign_process: "本机回环路由端口由其他程序占用，未执行清理。",
  managed_gateway_cleanup_active_instances: "仍有隔离实例正在运行，请先停止后再清理旧 Gateway。",
  managed_gateway_cleanup_failed: "旧 Gateway 未能完全退出，请停止隔离实例后重试。",
  managed_gateway_cleanup_process_lookup_failed: "无法检查占用回环端口的进程，未执行清理。",
  managed_gateway_cleanup_unsupported_platform: "当前平台不支持自动清理旧 Gateway。",
  managed_gateway_sidecar_missing: "找不到 R Codex 本地路由服务程序，无法启动 Gateway。",
  managed_gateway_sidecar_spawn_failed: "无法启动 R Codex 本地路由服务。",
  managed_gateway_credential_timeout: "本机凭据库授权超时；请确认 R Codex 已获准访问 Provider API Key 后重试。",
  managed_gateway_credential_worker_failed: "本机凭据库读取进程异常结束；请重试本次启动。",
  codex_profile_probe_failed: "当前 Codex 版本无法加载生成的受管理 Profile，已阻止启动。",
  codex_current_config_invalid: "当前 Codex config.toml 无法安全解析或文件过大；请先修复配置后再启动。",
  codex_catalog_validation_failed: "当前 Codex 版本无法加载生成的模型目录，已阻止启动。",
  history_provider_target_invalid: "目标 Provider 名称格式无效；只能使用 1–64 位字母、数字、点、下划线或连字符。",
  history_provider_source_invalid: "旧 Provider 名称格式无效；请检查输入内容。",
  history_provider_sources_empty: "请至少输入一个需要修复的旧 Provider 名称。",
  history_provider_target_missing: "目标 Provider 未在所选环境的 Codex 配置中定义；请先添加对应服务商定义并重新扫描，或选择已配置的名称。",
  history_repair_busy: "检测到 Codex 正在运行或有配置操作占用，请退出 Codex App 和 CLI 后重试；R Codex 不会自动关闭它们。",
  history_repair_conflict: "预览后会话、配置或索引已变化。请重新扫描；恢复时出现此提示会保留现有数据和备份。",
  history_repair_stop_confirmation_required: "请先退出 Codex App 和 CLI，并确认修复期间不重新启动。",
  history_repair_recovery_required: "有未完成的修复，请从备份列表恢复后再继续。",
  history_repair_process_check_failed: "无法核验 Codex 是否已停止，已取消写入。请检查进程访问权限后重试。",
  history_repair_database_busy: "会话数据库正在被使用，未执行修复。请退出 Codex 后重试。",
  history_repair_database_unreadable: "无法读取会话数据库，请检查文件权限或数据库完整性。",
  history_repair_database_write_failed: "数据库写入失败。请查看备份状态；未完成的修复可从备份恢复。",
  history_repair_rollout_invalid: "发现损坏或无法识别的会话文件，已阻止修复。原文件保持不变。",
  history_repair_schema_unknown: "当前会话数据库结构不受支持，已阻止修复以保留原数据。",
  history_repair_local_host_unknown: "无法确定会话目录中的本机记录，已阻止修复，避免影响远程会话。",
  history_repair_config_invalid: "Codex 配置无法解析，或配置字段结构无效。请先修复 config.toml。",
  history_repair_scope_unavailable: "所选 Codex 环境不存在或已不可用，请重新打开修复面板。",
  history_repair_symlink: "会话或数据库包含链接，当前修复不处理链接文件。请确认环境目录后再操作。",
  history_repair_limit_exceeded: "会话文件或记录超过当前扫描限制，已取消修复。",
  history_repair_backup_unreadable: "修复备份缺失、损坏或已被修改，未覆盖当前数据。",
  history_repair_backup_failed: "无法完成修复备份，未开始写入会话。请检查可用空间和目录权限。",
  history_repair_read_failed: "无法读取会话文件，请检查目录和权限。",
  history_repair_write_failed: "无法写入修复文件，请检查空间和权限，并查看备份状态。",
  history_repair_verification_failed: "修复后校验失败，请查看备份状态并恢复。",
  history_repair_worker_failed: "会话修复任务异常结束，请重新打开面板检查备份状态。",

  codex_catalog_models_missing: "生成的模型目录未被当前 Codex 完整识别，已阻止启动。",
  instance_not_terminal: "只有已退出或启动失败的实例才能删除；请先停止或重新核验当前实例。",
  instance_lock_active: "实例配置锁仍处于活动状态，请先重新核验并确认实例已退出。",
  instance_lifecycle_path_invalid: "实例生命周期目录不属于 R Codex 管理范围，已拒绝删除。",
  managed_app_data_shared: "还有其他 R Codex App 实例记录正在使用共享 App 数据，请先清除其他结束实例后重试。",
  managed_instance_storage_cleanup_failed: "实例专属目录清理失败，实例记录已保留，请检查目录权限后重试。",
  managed_sqlite_home_cleanup_failed: "实例对话数据库清理失败，实例记录已保留，请检查目录权限后重试。",
  managed_electron_data_cleanup_failed: "隔离 App 数据清理失败，实例记录已保留，请检查目录权限后重试。",
  managed_app_preset_missing: "实例缺少启动方案信息，已阻止删除以避免误删其他隔离数据。",
  codex_instance_not_found: "实例记录不存在，可能已被删除；请刷新列表。",
};

function parseLegacyError(error: string): CommandError {
  const separator = error.indexOf(":");
  const code = separator === -1 ? error : error.slice(0, separator);
  const suffix = separator === -1 ? "" : error.slice(separator + 1);
  if (Object.hasOwn(legacyErrorMessages, code)) {
    const conflictingInstances = code === "configuration_in_use"
      ? suffix.split(",").map((instanceId) => instanceId.trim()).filter(Boolean).map((instanceId) => ({ instanceId, state: "running" as const }))
      : [];
    return { code, message: legacyErrorMessages[code], conflictingInstances };
  }
  return { code: "legacy_command_failed", message: error, conflictingInstances: [] };
}

export function toCommandError(error: unknown): CommandError {
  let candidate: unknown = error;
  if (typeof error === "string") {
    try { candidate = JSON.parse(error); } catch { return parseLegacyError(error); }
  }
  if (isRecord(candidate)) {
    const nested = isRecord(candidate.error) ? candidate.error : candidate;
    const code = typeof nested.code === "string" ? nested.code : "command_failed";
    const message = typeof nested.message === "string" ? nested.message : legacyErrorMessages[code] ?? code;
    const conflicts = nested.conflictingInstances ?? nested.instances ?? nested.instanceIds;
    return { code, message, conflictingInstances: normalizeLocks(conflicts) };
  }
  return { code: "command_failed", message: "操作失败，请刷新状态后重试。", conflictingInstances: [] };
}

function commandUnavailable(error: unknown): boolean {
  const parsed = toCommandError(error);
  if (parsed.code === "command_not_found" || parsed.code === "unsupported_command") return true;
  const message = parsed.message.toLowerCase();
  return message.includes("not found") || message.includes("unknown command") || message.includes("not registered");
}


export type LaunchTarget = "cli" | "managed_app";

export function isManagedAppTarget(target: string | null | undefined) {
  return target === "managed_app";
}

export function isKnownLaunchTarget(target: string | null | undefined) {
  return target === "managed_app" || target === "cli";
}

export function launchTargetLabel(target: string | null | undefined) {
  return isManagedAppTarget(target) ? "隔离 Codex App" : "隔离 Codex CLI";
}

export function findManagedWorkspace<T extends { entryKind?: string | null }>(presets: readonly T[]) {
  return presets.find((preset) => preset.entryKind === "managed_app");
}

export function isOfficialCredential(kind: CredentialKind) {
  return kind === "official";
}

export function isApiKeyCredential(kind: CredentialKind) {
  return kind === "api_key";
}

export function credentialKindLabel(kind: CredentialKind, officialLabel = "官方") {
  if (kind === "official") return officialLabel;
  if (kind === "none") return "未配置渠道";
  return "API Key";
}

export function isDirectTransport(mode: string | null | undefined) {
  return mode === "direct_responses";
}

export function isGatewayTransport(mode: string | null | undefined) {
  return mode === "gateway_passthrough" || mode === "gateway_chat_completions";
}

export function usesOfficialModel(preset: { credentialKind: CredentialKind; models: readonly { sourceKind: string }[] }) {
  return isOfficialCredential(preset.credentialKind) || preset.models.some((model) => model.sourceKind === "official");
}

export function isActiveInstanceState(state: InstanceState): boolean {
  return activeStates.has(state);
}

/** 当前隔离实例仍在活动状态时，工作区可以改，下次启动才用新配置。 */
export function runningWorkspaceRestartNotice(
  instances: readonly Pick<RuntimeInstanceSummary, "launchPresetId" | "state">[],
  presetId: string,
) {
  if (!instances.some((instance) => instance.launchPresetId === presetId && isActiveInstanceState(instance.state))) return undefined;
  return "当前隔离实例正在运行，重启可应用修改";
}

/** 渠道凭据仍被某个运行中的隔离实例占用时，删除保持禁用。 */
export function runningProviderLockReason(locks: readonly LockedInstanceRef[]) {
  if (!locks.some((lock) => isActiveInstanceState(lock.state))) return undefined;
  return "有隔离实例正在使用这个渠道，停止对应实例后才能删除。";
}

export async function getCodexControlSnapshot(): Promise<CodexControlSnapshot> {
  const [snapshotResult, providersResult, compositionsResult] = await Promise.allSettled([
    invoke<RawSnapshot>("get_codex_control_snapshot"),
    invoke<RawProvider[]>("list_api_key_providers"),
    invoke<RawManagedComposition[]>("get_managed_launch_preset_compositions"),
  ]);
  const listedProviders = providersResult.status === "fulfilled" ? providersResult.value : [];
  const compositions = compositionsResult.status === "fulfilled" ? compositionsResult.value : [];
  if (snapshotResult.status === "fulfilled") {
    return normalizeSnapshot(snapshotResult.value, listedProviders, compositions);
  }
  if (commandUnavailable(snapshotResult.reason) && providersResult.status === "fulfilled") {
    return normalizeSnapshot({
      presets: listedProviders.map((provider) => ({
        id: `legacy-provider:${provider.id}`,
        name: provider.name,
        version: 0,
        credentialKind: "api_key",
        credentialId: provider.id,
        credentialLabel: provider.name,
        transportMode: provider.transportMode,
        isPreferred: provider.isDefault,
        lockedByInstances: [],
        compatibilityFallback: true,
        providerIds: [provider.id],
        models: [],
      })),
      compatibility: {
        controlPlaneAvailable: false,
        officialAccountsAvailable: false,
        managedAppAvailable: false,
        officialHybridAvailable: false,
        code: "control_plane_unavailable",
        message: "当前后端尚未启用官方账户、Codex 启动方案和运行实例控制面。API Key 模型渠道仍可使用。",
      },
    }, listedProviders);
  }
  throw snapshotResult.reason;
}

export async function getCodexSpecialModelRoles(): Promise<SpecialModelRoleSummary[]> {
  return invoke<SpecialModelRoleSummary[]>("get_codex_special_model_roles");
}

export async function invokeControl<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  return invoke<T>(command, args);
}

export function isUnavailableCommand(error: unknown): boolean {
  return commandUnavailable(error);
}
