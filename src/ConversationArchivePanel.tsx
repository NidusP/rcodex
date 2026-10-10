import { invoke } from "@tauri-apps/api/core";
import { useEffect, useMemo, useState } from "react";
import type { ArchiveBodyPage, ArchiveRequestDetail, ArchiveRequestSummary, ArchiveStatus } from "./conversation-archive";
import { HintedButton } from "./components/hinted-button";
import { Checkbox, Select, Tabs } from "@mantine/core";
import "./ConversationArchivePanel.css";

const stageLabels: Record<string, string> = {
  ingress: "Codex 请求",
  "upstream-request": "渠道请求",
  "upstream-response": "渠道响应",
  "downstream-response": "Codex 响应",
};

function requestGroup(request: ArchiveRequestSummary) {
  return `${request.scopeId}/${request.conversationKey ?? "pending"}`;
}

export default function ConversationArchivePanel({
  scopeId,
}: {
  scopeId?: string;
} = {}) {
  const [status, setStatus] = useState<ArchiveStatus | null>(null);
  const [requests, setRequests] = useState<ArchiveRequestSummary[]>([]);
  const [detail, setDetail] = useState<ArchiveRequestDetail | null>(null);
  const [bodies, setBodies] = useState<Record<string, ArchiveBodyPage>>({});
  const [stage, setStage] = useState("");
  const [compareStage, setCompareStage] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [confirmClear, setConfirmClear] = useState(false);
  const visibleRequests = useMemo(
    () =>
      scopeId
        ? requests.filter((request) => request.scopeId === scopeId)
        : requests,
    [requests, scopeId],
  );
  const groups = useMemo(() => {
    const grouped = new Map<string, ArchiveRequestSummary[]>();
    for (const request of visibleRequests) {
      const key = requestGroup(request);
      grouped.set(key, [...(grouped.get(key) ?? []), request]);
    }
    return [...grouped.entries()];
  }, [visibleRequests]);

  async function run(action: () => Promise<void>) {
    setBusy(true);
    setError(null);
    try { await action(); }
    catch { setError("请求档案操作失败，请检查本机网关和存储状态后重试。"); }
    finally { setBusy(false); }
  }

  async function refresh() {
    const [nextStatus, nextRequests] = await Promise.all([
      invoke<ArchiveStatus>("conversation_archive_status"),
      invoke<ArchiveRequestSummary[]>("conversation_archive_list", { limit: 50 }),
    ]);
    setStatus(nextStatus);
    setRequests(nextRequests);
  }

  useEffect(() => { void run(refresh); }, []);

  async function openRequest(requestId: string) {
    const next = await invoke<ArchiveRequestDetail | null>("conversation_archive_detail", { requestId });
    if (!next) throw new Error("archive_request_not_found");
    setDetail(next);
    setBodies({});
    setCompareStage("");
    const firstStage = next.stages[0]?.stage ?? "";
    setStage(firstStage);
    if (firstStage) await readBody(requestId, firstStage, 0, {});
  }

  async function readBody(requestId: string, nextStage: string, offset = 0, current = bodies) {
    const page = await invoke<ArchiveBodyPage>("conversation_archive_body", {
      requestId, stage: nextStage, offset, limit: 64 * 1024,
    });
    const previous = current[nextStage];
    setBodies((value) => ({
      ...value,
      [nextStage]: offset && previous ? { ...page, text: previous.text + page.text, offset: previous.offset } : page,
    }));
  }

  async function chooseStage(nextStage: string) {
    if (!detail) return;
    setStage(nextStage);
    if (nextStage === compareStage) setCompareStage("");
    if (!bodies[nextStage]) await readBody(detail.request.requestId, nextStage);
  }

  async function chooseComparison(nextStage: string) {
    if (!detail) return;
    setCompareStage(nextStage);
    if (nextStage && !bodies[nextStage]) await readBody(detail.request.requestId, nextStage);
  }

  function bodyView(selectedStage: string) {
    const body = bodies[selectedStage];
    if (!body) return <p className="archive-empty">正在读取阶段正文…</p>;
    return <div><pre className="archive-body" tabIndex={0}>{body.text || "（空正文）"}</pre><div className="archive-detail-meta"><span>{stageLabels[selectedStage] ?? selectedStage}</span><span>字节 {body.offset}–{body.nextOffset} / {body.totalBytes}</span></div>{body.nextOffset < body.totalBytes && detail && <HintedButton className="secondary-action" disabled={busy} onClick={() => void run(() => readBody(detail.request.requestId, selectedStage, body.nextOffset))}>继续加载</HintedButton>}</div>;
  }

  return <section className="conversation-archive" aria-label="完整请求档案" aria-busy={busy}>
    <div className="archive-heading"><div><h3>{scopeId ? "这个实例的完整请求档案" : "完整请求档案"}</h3><small>{status ? `${status.requestCount} 条请求 · ${status.stageCount} 个阶段 · ${(status.storedBytes / 1024).toFixed(1)} KB` : "读取中"}</small></div><div className="archive-actions"><Checkbox className="archive-toggle" label="记录正文" checked={status?.enabled ?? false} disabled={busy || !status} onChange={(event) => { const enabled = event.currentTarget.checked; void run(async () => { setStatus(await invoke<ArchiveStatus>("conversation_archive_set_policy", { enabled })); }); }} /><HintedButton className="secondary-action" disabled={busy} onClick={() => void run(refresh)}>刷新</HintedButton></div></div>
    <p>显式开启后，保存经过 R Codex 网关的四阶段正文，可能包含提示词、代码和附件内容。档案仅保存在本机，不进入普通诊断。</p>
    {status?.enabled && <small className="archive-storage">关闭记录后才能清空；停止记录不会删除现有档案。</small>}
    {!scopeId && <HintedButton className="danger-action" disabled={busy || !status?.requestCount || status.enabled} title={status?.enabled ? "请先关闭完整请求记录" : "清空本机完整请求档案"} onClick={() => setConfirmClear(true)}>清空完整档案</HintedButton>}
    {confirmClear && <div className="archive-confirm" role="alert"><p>将永久删除本机完整请求档案；原生会话与普通诊断不受影响。</p><div><HintedButton className="secondary-action" disabled={busy} onClick={() => setConfirmClear(false)}>取消</HintedButton><HintedButton className="danger-action" disabled={busy} onClick={() => void run(async () => { await invoke("conversation_archive_clear"); setDetail(null); setBodies({}); setConfirmClear(false); await refresh(); })}>确认清空</HintedButton></div></div>}
    {groups.length === 0 && status && <p className="archive-empty">{scopeId ? "这个实例还没有完整请求档案。开启记录正文后，从这个实例发出的请求会保存在这里。" : "尚无请求档案。开启记录后，从 R Codex 启动 Codex 并发送请求。"}</p>}
    <div className="archive-groups">{groups.map(([group, entries]) => <section className="archive-group" key={group}><div className="archive-group-button"><span><strong>{entries[0].conversationKey ? `会话 ${entries[0].conversationKey}` : "会话待关联"}</strong><small>{entries[0].scopeId === "default" ? "旧版档案" : "R Codex 工作区"} · {entries.length} 条请求</small></span></div><div className="archive-requests">{entries.map((request) => <article className="archive-request" key={request.requestId}><HintedButton className="archive-request-button" disabled={busy} aria-expanded={detail?.request.requestId === request.requestId} onClick={() => void run(() => openRequest(request.requestId))}><span>{new Date(request.createdAt * 1000).toLocaleString()} · {request.partial ? "部分记录" : `${request.stages} 个阶段`}</span><small>{request.captureStatus} · {request.correlationStatus}</small></HintedButton>{detail?.request.requestId === request.requestId && <div className="archive-detail"><Tabs className="archive-stage-tabs" variant="pills" value={stage} onChange={(value) => { if (value) void run(() => chooseStage(value)); }}><Tabs.List>{detail.stages.map((item) => <Tabs.Tab value={item.stage} key={item.stage} disabled={busy}>{stageLabels[item.stage] ?? item.stage}{item.partial ? " · 不完整" : ""}</Tabs.Tab>)}</Tabs.List></Tabs><div className="archive-compare"><Select label="阶段对照" placeholder="不对照" data={[{ value: "__rcodex_empty__", label: "不对照" }, ...detail.stages.filter((item) => item.stage !== stage).map((item) => ({ value: item.stage, label: stageLabels[item.stage] ?? item.stage }))]} value={compareStage ? compareStage : "__rcodex_empty__"} disabled={busy} onChange={(value) => void run(() => chooseComparison(!value || value === "__rcodex_empty__" ? "" : value))} allowDeselect={false} withCheckIcon={false} comboboxProps={{ withinPortal: true, position: "bottom-start", middlewares: { flip: true, shift: true }, zIndex: 400 }} /></div><div className={compareStage ? "archive-body-pair" : ""}>{stage && bodyView(stage)}{compareStage && bodyView(compareStage)}</div><code>{JSON.stringify(detail.metadata)}</code></div>}</article>)}</div></section>)}</div>
    {error && <p role="alert" className="archive-error">{error}</p>}
  </section>;
}
