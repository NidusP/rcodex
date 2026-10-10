import { For, Show, createSignal } from "solid-js";

const flows = [
  {
    id: "default",
    label: "默认 App",
    title: "保留原生历史",
    copy: "R Codex 把模型路由投影到默认配置，App 仍使用自己的会话、认证和窗口数据。",
    steps: ["选择 Provider", "应用模型配置", "启动或重启默认 App", "请求进入本机 Gateway"],
  },
  {
    id: "gateway",
    label: "Gateway",
    title: "按模型精确路由",
    copy: "本机 Gateway 只处理回环请求，按公共模型 ID 找到固定 Provider 修订，再把请求转发到真实上游。",
    steps: ["Codex 请求公共模型 ID", "Gateway 校验 binding", "匹配 RouteSet 快照", "转发并记录脱敏诊断"],
  },
  {
    id: "isolated",
    label: "隔离环境",
    title: "独立目录，等待真实验收",
    copy: "隔离 App 的目录、签名、历史和 Gateway 边界已经实现；真实桌面授权、连续会话与跨平台发布仍在验收。",
    steps: ["独立 CODEX_HOME", "独立会话 SQLite", "独立进程与实例锁", "完成实机验收"],
  },
];

export default function FlowSwitcher() {
  const [active, setActive] = createSignal("default");
  const current = () => flows.find((flow) => flow.id === active()) ?? flows[0];

  return (
    <div class="flow-demo" aria-label="R Codex 工作方式示意">
      <div class="flow-tabs" role="tablist" aria-label="工作方式">
        <For each={flows}>
          {(flow) => (
            <button
              classList={{ active: active() === flow.id }}
              type="button"
              role="tab"
              aria-selected={active() === flow.id}
              onClick={() => setActive(flow.id)}
            >
              {flow.label}
            </button>
          )}
        </For>
      </div>
      <div class="flow-copy" role="tabpanel">
        <div>
          <span class="flow-kicker">{current().label}</span>
          <h3>{current().title}</h3>
          <p>{current().copy}</p>
        </div>
        <ol class="flow-steps">
          <For each={current().steps}>
            {(step, index) => <li><span>{index() + 1}</span>{step}</li>}
          </For>
        </ol>
      </div>
      <Show when={active() === "isolated"}>
        <p class="flow-note">当前版本：隔离 App 已实现，真实桌面与发布合同仍未完成。</p>
      </Show>
    </div>
  );
}
