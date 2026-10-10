import { useState } from "react";
import { List } from "@mantine/core";
import { formatTime } from "../../app-model";
import { HintedButton } from "../hinted-button";
import { useAppState } from "../app-state";
import ConversationArchivePanel from "../../ConversationArchivePanel";
import { ControlDrawer } from "./drawer-layout";
import "./diagnostics-drawer.css";

/**
 * @component DiagnosticsDrawer
 * @description 当前实例的请求记录。可以从一条记录进入这个实例的完整请求档案。
 */
export function DiagnosticsDrawer() {
  const {
    utilityDrawer,
    setUtilityDrawer,
    presets,
    scopedPresetId,
    requestModels,
    providerNames,
    visibleManagedInstances,
    uiBusy,
  } = useAppState();
  const [archiveOpen, setArchiveOpen] = useState(false);
  const preset = presets.find((item) => item.id === scopedPresetId);
  const instanceIds = new Set(
    visibleManagedInstances
      .filter((instance) => instance.launchPresetId === scopedPresetId)
      .map((instance) => instance.id),
  );
  const requests = requestModels.filter(
    (event) => event.instanceId !== null && instanceIds.has(event.instanceId),
  );
  return (
    <ControlDrawer
      opened={utilityDrawer === "diagnostics"}
      onClose={() => {
        setArchiveOpen(false);
        setUtilityDrawer(null);
      }}
      title={archiveOpen ? "完整请求档案" : "请求记录"}
      size="min(960px, 100%)"
      closeDisabled={uiBusy}
    >
      <p className="drawer-intro">
        {preset
          ? "只显示当前实例“" +
            preset.name +
            "”经过本机 Gateway 的请求。"
          : "请先选择一个实例。"}
      </p>
      {archiveOpen ? (
        <>
          <div className="drawer-toolbar">
            <HintedButton
              className="secondary-action"
              type="button"
              onClick={() => setArchiveOpen(false)}
            >
              返回请求记录
            </HintedButton>
          </div>
          <ConversationArchivePanel scopeId={scopedPresetId ?? undefined} />
        </>
      ) : requests.length === 0 ? (
        <p className="empty-state">这个实例还没有请求记录。</p>
      ) : (
        <List className="diagnostic-list" listStyleType="none" spacing={6}>
          {requests.map((event) => (
            <List.Item key={"request-" + event.id}>
              <div className="diagnostic-row">
                <span
                  className={
                    "outcome " +
                    (event.outcome === "success" ? "success" : "failed")
                  }
                >
                  {event.outcome === "success" ? "成功" : "失败"}
                </span>
                <div>
                  <strong>{event.publicModelId || "未指定模型"}</strong>
                  <small>
                    {providerNames[event.providerId] ?? event.providerId} · 上游{" "}
                    {event.upstreamModelId || "—"} · {formatTime(event.createdAt)}
                  </small>
                  <small>
                    {event.wireApi}
                    {event.stream ? " · 流式" : ""}
                  </small>
                </div>
                <div className="diagnostic-meta">
                  <strong>{event.httpStatus ?? "网络错误"}</strong>
                  <small>
                    {event.errorCode ? "R Codex：" + event.errorCode : "请求已完成"}
                    {event.totalLatencyMs === null
                      ? ""
                      : " · " + event.totalLatencyMs + "ms"}
                  </small>
                </div>
                {event.errorDetail && (
                  <code title="脱敏后的上游错误摘要。">上游：{event.errorDetail}</code>
                )}
                <HintedButton
                  className="secondary-action"
                  type="button"
                  disabled={!scopedPresetId}
                  onClick={() => setArchiveOpen(true)}
                >
                  完整档案
                </HintedButton>
              </div>
            </List.Item>
          ))}
        </List>
      )}
    </ControlDrawer>
  );
}
