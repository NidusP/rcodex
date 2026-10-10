import type { GatewayTimeoutSettings } from "../../app-model";

/** 渠道探针结果：协议、模型列表和错误码。 */
export type DraftChannelProbe = {
  probeTicketId: string;
  status: "ready" | "degraded" | "unreachable" | "unauthorized";
  wireApi: "responses" | "chat_completions" | null;
  models: {
    upstreamModelId: string;
    displayName: string;
    ownedBy: string | null;
  }[];
  modelsCheck: "ready" | "failed" | "not_run";
  errorCode: string | null;
};

/** 保存新模型渠道时提交的表单草稿。 */
export type ProviderChannelDraft = {
  name: string;
  baseUrl: string;
  apiKey: string;
  gatewaySettings: GatewayTimeoutSettings;
  probeTicketId?: string;
};
