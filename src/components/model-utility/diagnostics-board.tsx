import { useState } from "react";
import { Checkbox, List, Select, Tabs, TextInput } from "@mantine/core";
import {
  type DiagnosticEventModel,
  type RequestEventModel,
  formatTime,
} from "../../app-model";
import { HintedButton } from "../hinted-button";
import ConversationArchivePanel from "../../ConversationArchivePanel";
import "./diagnostics-board.css";

function recoveryActions(event: DiagnosticEventModel) {
  if (event.outcome !== "failed") return [] as Array<"test" | "instances">;
  const code = (event.errorCode ?? "").toLowerCase();
  if (event.source === "probe") return ["test"] as const;
  if (
    code.includes("instance") ||
    code.includes("route") ||
    code.includes("model")
  )
    return ["instances"] as const;
  return [] as const;
}

/**
 * @component DiagnosticsBoard
 * @description 诊断内容：事件、请求记录和完整档案。供诊断抽屉展示脱敏记录。
 */
export function DiagnosticsBoard({
  events,
  requests,
  providerNames,
  nextStep,
  onTestProvider,
  onViewInstances,
}: {
  events: DiagnosticEventModel[];
  requests: RequestEventModel[];
  providerNames: Record<string, string>;
  nextStep: (event: DiagnosticEventModel) => string;
  onTestProvider: (providerId: string) => void;
  onViewInstances: () => void;
}) {
  const [tab, setTab] = useState<"events" | "requests" | "archive">("events");
  const [failedOnly, setFailedOnly] = useState(false);
  const [providerId, setProviderId] = useState("");
  const [modelQuery, setModelQuery] = useState("");
  const query = modelQuery.trim().toLowerCase();
  const visibleEvents = events.filter((event) => {
    if (failedOnly && event.outcome !== "failed") return false;
    if (providerId && event.providerId !== providerId) return false;
    if (
      query &&
      !`${event.model ?? ""} ${event.route} ${event.errorCode ?? ""}`
        .toLowerCase()
        .includes(query)
    )
      return false;
    return true;
  });
  const visibleRequests = requests.filter((event) => {
    if (failedOnly && event.outcome === "success") return false;
    if (providerId && event.providerId !== providerId) return false;
    if (
      query &&
      !`${event.publicModelId} ${event.upstreamModelId} ${event.errorCode ?? ""}`
        .toLowerCase()
        .includes(query)
    )
      return false;
    return true;
  });
  const providerOptions = [
    ...new Set([
      ...events.map((event) => event.providerId),
      ...requests.map((event) => event.providerId),
    ]),
  ];
  return (
    <div className="diagnostics-board">
      <Tabs
        className="surface-tabs"
        variant="pills"
        value={tab}
        onChange={(value) => {
          if (value === "events" || value === "requests" || value === "archive") {
            setTab(value);
          }
        }}
      >
        <Tabs.List aria-label="诊断内容">
          <Tabs.Tab value="events">事件</Tabs.Tab>
          <Tabs.Tab value="requests">请求记录</Tabs.Tab>
          <Tabs.Tab value="archive">完整档案</Tabs.Tab>
        </Tabs.List>
      </Tabs>
      {tab !== "archive" && (
        <div className="diagnostic-filters">
          <Checkbox
            label="仅失败"
            checked={failedOnly}
            onChange={(event) => setFailedOnly(event.currentTarget.checked)}
          />
          <Select
            aria-label="按渠道筛选"
            placeholder="全部渠道"
            data={[
              { value: "__rcodex_empty__", label: "全部渠道" },
              ...providerOptions.map((id) => ({
                value: id,
                label: providerNames[id] ?? id,
              })),
            ]}
            value={
              providerId === ""
                ? "__rcodex_empty__"
                : providerOptions.includes(providerId)
                  ? providerId
                  : null
            }
            onChange={(next) =>
              setProviderId(!next || next === "__rcodex_empty__" ? "" : next)
            }
            allowDeselect={false}
            withCheckIcon={false}
            comboboxProps={{
              withinPortal: true,
              position: "bottom-start",
              middlewares: { flip: true, shift: true },
              zIndex: 400,
            }}
          />
          <TextInput
            aria-label="按模型筛选"
            value={modelQuery}
            placeholder="模型或错误码"
            onChange={(event) => setModelQuery(event.currentTarget.value)}
          />
        </div>
      )}
      {tab === "events" &&
        (visibleEvents.length === 0 ? (
          <p className="empty-state">没有符合筛选的诊断事件。</p>
        ) : (
          <List className="diagnostic-list" listStyleType="none" spacing={6}>
            {visibleEvents.map((event) => (
              <List.Item key={event.id}>
                <div className="diagnostic-row">
                  <span className={`outcome ${event.outcome}`}>
                    {event.outcome === "success" ? "成功" : "失败"}
                  </span>
                  <div>
                    <strong>
                      {event.source === "probe" ? "模型渠道检测" : "Gateway 请求"}{" "}
                      · {providerNames[event.providerId] ?? "模型渠道"}
                    </strong>
                    <small>
                      {formatTime(event.createdAt)} · {event.route} ·{" "}
                      {event.model ?? "未指定模型"}
                      {event.stream ? " · 流式" : ""}
                    </small>
                    <small className="diagnostic-next-step">
                      下一步：{nextStep(event)}
                    </small>
                    <div className="diagnostic-recovery">
                      {recoveryActions(event).map((action) => (
                        <HintedButton
                          key={action}
                          className="text-button"
                          type="button"
                          onClick={() => {
                            if (action === "test") onTestProvider(event.providerId);
                            if (action === "instances") onViewInstances();
                          }}
                        >
                          {action === "test" ? "重新检测" : "查看实例"}
                        </HintedButton>
                      ))}
                    </div>
                  </div>
                  <div className="diagnostic-meta">
                    <strong>{event.httpStatus ?? "网络错误"}</strong>
                    <small>
                      {event.latencyMs === null ? "—" : `${event.latencyMs}ms`}
                    </small>
                  </div>
                  {event.errorCode && <code>{event.errorCode}</code>}
                </div>
              </List.Item>
            ))}
          </List>
        ))}
      {tab === "requests" &&
        (visibleRequests.length === 0 ? (
          <p className="empty-state">没有符合筛选的请求记录。</p>
        ) : (
          <List className="diagnostic-list" listStyleType="none" spacing={6}>
            {visibleRequests.map((event) => (
              <List.Item key={`request-${event.id}`}>
                <div className="diagnostic-row">
                  <span
                    className={`outcome ${event.outcome === "success" ? "success" : "failed"}`}
                  >
                    {event.outcome === "success" ? "成功" : "失败"}
                  </span>
                  <div>
                    <strong>{event.publicModelId || "未指定模型"}</strong>
                    <small>
                      {providerNames[event.providerId] ?? event.providerId} · 上游{" "}
                      {event.upstreamModelId || "—"} ·{" "}
                      {formatTime(event.createdAt)}
                    </small>
                    <small>
                      {event.instanceId ? `实例 ${event.instanceId}` : "隔离路由"}{" "}
                      · {event.wireApi}
                      {event.stream ? " · 流式" : ""}
                    </small>
                  </div>
                  <div className="diagnostic-meta">
                    <strong>{event.httpStatus ?? "网络错误"}</strong>
                    <small>
                      {event.errorCode ? `R Codex：${event.errorCode}` : "请求已完成"}
                      {event.totalLatencyMs === null
                        ? ""
                        : ` · ${event.totalLatencyMs}ms`}
                    </small>
                  </div>
                  {event.errorDetail && (
                    <code title="脱敏后的真实上游地址与限长错误摘要。Codex 看到的 127.0.0.1 只是本机 Gateway。">
                      上游：{event.errorDetail}
                    </code>
                  )}
                </div>
              </List.Item>
            ))}
          </List>
        ))}
      {tab === "archive" && <ConversationArchivePanel />}
    </div>
  );
}
