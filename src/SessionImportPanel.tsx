import { invoke } from "@tauri-apps/api/core";
import { useState } from "react";
import { Download, RefreshCw } from "lucide-react";
import { HintedButton } from "./components/hinted-button";
import { Checkbox, List, Select } from "@mantine/core";
import "./SessionImportPanel.css";

type ImportableSession = { id: string; title: string; provider: string; createdAt: number };
type ImportableProject = { cwd: string; sessionCount: number; latestTitle: string; sessions: ImportableSession[] };
type ImportResult = { copied: number; skipped: number; conflicted: number };

const errors: Record<string, string> = {
  resource_operation_in_progress: "正在启动 Codex 或导入会话，请稍后重试。",
  session_import_busy: "请先退出我的 Codex 和 R Codex 启动的 Codex App、CLI，再重试导入。",
  session_import_process_check_failed: "无法确认 Codex 已退出，本次未执行导入。",
  session_import_source_changed: "源会话仍在变化，请退出 Codex 后重新扫描。",
  session_import_project_changed: "项目会话已变化，请重新扫描后选择项目。",
  session_import_schema_mismatch: "两边的会话索引版本不匹配，请用同一版本 Codex 打开两边环境后退出并重试。",
  isolated_home_not_ready: "请先从 R Codex 启动一次 Codex，完成初始化并退出后再导入。",
  managed_app_in_use: "请先退出 R Codex App，再导入项目会话。",
  managed_app_data_shared: "当前仍有其他 R Codex App 运行记录，暂不能清除共享 App 数据。",
  session_import_target_database_not_ready: "R Codex 的会话索引尚未就绪，请先启动 R Codex 创建一个会话，再退出并重试。",
  session_import_source_path_rejected: "会话路径包含链接或超出允许目录，已停止导入。",
  session_import_refuses_default_home: "目标目录与我的 Codex 重叠，已阻止写入。",
  session_import_source_unreadable: "无法读取会话索引或文件，请检查权限后重新扫描。",
  session_import_invalid_metadata: "会话文件的身份信息无效，本次导入已撤销。",
  session_import_write_failed: "无法写入 R Codex 会话，本次导入已撤销，请检查磁盘空间和目录权限。",
};

function errorMessage(cause: unknown) {
  return typeof cause === "string" ? errors[cause] ?? "操作失败，请检查会话文件和目录权限后重试。" : "操作失败，请稍后重试。";
}

export default function SessionImportPanel({ launchPresetId, disabled }: { launchPresetId?: string; disabled?: boolean }) {
  const [projects, setProjects] = useState<ImportableProject[]>([]);
  const [selected, setSelected] = useState("");
  const [confirmed, setConfirmed] = useState(false);
  const [busy, setBusy] = useState(false);
  const [scanned, setScanned] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const project = projects.find((item) => item.cwd === selected);

  async function scan() {
    if (busy) return;
    setBusy(true);
    setError(null);
    setMessage(null);
    setSelected("");
    setConfirmed(false);
    try {
      setProjects(await invoke<ImportableProject[]>("list_importable_session_projects"));
      setScanned(true);
    } catch (cause) {
      setProjects([]);
      setScanned(false);
      setError(errorMessage(cause));
    } finally {
      setBusy(false);
    }
  }

  async function apply() {
    if (!launchPresetId || !project || !confirmed || busy) return;
    setBusy(true);
    setError(null);
    setMessage(null);
    try {
      const result = await invoke<ImportResult>("import_sessions_from_default", {
        input: { launchPresetId, cwd: selected, confirmedStopped: confirmed },
      });
      setMessage(`已复制 ${result.copied} 个会话到 R Codex${result.skipped ? `，跳过 ${result.skipped} 个未变化会话` : ""}${result.conflicted ? `，为 ${result.conflicted} 个有变化会话生成了新的隔离副本` : ""}。原副本和源会话保持原样，重新启动 R Codex 后可查看。`);
      setConfirmed(false);
    } catch (cause) {
      setError(errorMessage(cause));
    } finally {
      setBusy(false);
    }
  }

  return <section className="detail-block session-import-panel" aria-label="导入我的 Codex 会话" aria-busy={busy}>
    <div className="detail-block-heading"><strong>导入项目会话</strong><span>我的 Codex → R Codex</span></div>
    <p>主动扫描后，选择一个项目，把活动会话单向复制到 R Codex。未变化的相同会话会跳过；源会话有变化时会生成新的隔离副本，保留原副本。原配置、登录和源会话保持原样。</p>
    <HintedButton className="secondary-action" type="button" disabled={busy || disabled} onClick={() => void scan()}>
      <RefreshCw size={14} aria-hidden="true" />{busy ? "正在处理…" : scanned ? "重新扫描" : "扫描我的 Codex 项目"}
    </HintedButton>
    {scanned && <div className="runtime-default-model"><Select label="导入项目目录" placeholder={projects.length ? "选择要导入的项目" : "没有可导入的项目"} data={projects.map((item) => ({ value: item.cwd, label: item.cwd + " · " + String(item.sessionCount) + " 个会话" }))} value={selected || null} disabled={Boolean(busy || disabled) || projects.length === 0} onChange={(value) => { setSelected(value ?? ""); setConfirmed(false); setMessage(null); }} allowDeselect={false} withCheckIcon={false} comboboxProps={{ withinPortal: true, position: "bottom-start", middlewares: { flip: true, shift: true }, zIndex: 400 }} /></div>}
    {project && <>
      <div className="session-import-preview" aria-label="待导入会话预览"><List listStyleType="none" spacing={6}>{project.sessions.map((session) => <List.Item key={session.id}><strong>{session.title || "未命名会话"}</strong><small>{new Date(session.createdAt * 1000).toLocaleString()} · 原渠道：{session.provider}</small></List.Item>)}</List></div>
      <p>会话内容会保留；跨模型渠道续聊可能仍受历史推理、工具和附件兼容性限制。项目中的附件文件仍需在本机原路径可用。</p>
      <Checkbox className="checkbox-label" checked={confirmed} disabled={busy || disabled} onChange={(event) => setConfirmed(event.currentTarget.checked)} label="我已退出我的 Codex 和 R Codex App、CLI" />
      <HintedButton className="secondary-action" type="button" disabled={busy || disabled || !launchPresetId || !confirmed} onClick={() => void apply()}><Download size={14} aria-hidden="true" />导入到 R Codex</HintedButton>
    </>}
    {error && <p className="session-import-error" role="alert">{error}</p>}
    {message && <p className="session-import-success" role="status">{message}</p>}
    {!launchPresetId && <small>请先配置并从 R Codex 启动一次 Codex，独立环境就绪后再导入。</small>}
  </section>;
}
