# 默认 App 回环 Gateway 与系统代理

> 历史记录（2026-09-22）：相关默认 App / 源历史修复入口已移除，不再执行本文的写入或恢复步骤。当前合同见 [独立工作区改造](designs/2026-09-21-isolated-only-product-plan.md)；不做默认配置清理向导。

> 状态：结论落档（2026-09-15）  
> 相关：[产品原理 §5.3.1](3rcodex-product-principles.md#531-默认-app-兼容模式)、[混合路由执行计划 · 回环代理](codex-hybrid-routing-execution-plan.md#2026-08-27-gateway-回环代理与实例退出收敛)、[README · 本地网关](../README.md#本地网关如何工作)

兼容配置把默认 Codex App 指到本机 Gateway（`http://127.0.0.1:43123/v1`）。**路由字段写在磁盘上，绕过系统代理的 `NO_PROXY` 只加在 3R 拉起的那一次进程里。** 在 Codex 提供稳定的配置级 `NO_PROXY` 之前，3R 支持的打通路径是：**从 3R Codex 安全启动默认 App**。Finder / Dock 直开不在 3R 能注入的环境里。

## 1. 产品结论

1. 空正文 `502 Unknown error`、`url: http://127.0.0.1:43123/...`、sidecar **没有** `event=forward`：请求被本机 HTTP 代理（Clash/mihomo 类，常见 `127.0.0.1:7892`）拦下，Gateway 没见到。不是上游 502，也不是实例头写错。
2. 3R 启动默认 App 会做两件事：把 Gateway 地址和准入头写入 `~/.codex/config.toml`；给**这一次** Codex 进程注入 `NO_PROXY`/`no_proxy`（保留已有条目并补齐 `127.0.0.1`、`localhost`、`::1`）。Finder 直开只有第一件。
3. Codex `config.toml` **没有** `no_proxy` / `http_proxy` 字段。`http_headers` 改不了系统代理这一跳。[issue #6060](https://github.com/openai/codex/issues/6060) 的 `http_proxy` 若落地，语义是**强制走正向代理**，会加重回环 502，不是绕过。
4. 3R 不改系统代理、不写 `~/.codex/.env`、不把 `openai_base_url` 换成自制域名、不给 LaunchServices 注入环境。那些要么越权，要么不可靠。
5. **日后若 Codex 支持配置级 `NO_PROXY`（或等价、且 Desktop/Finder 启动也读的 bypass）**，兼容物化把回环三项写进默认 home，Finder 直开才能在 3R 产品面上彻底打通。在此之前，3R 侧只能靠安全启动。

用户在 Clash / 系统「忽略这些主机」里**分条**排除 `127.0.0.1`、`localhost`、`::1` 仍然有效，但属于代理工具配置，不是 3R 功能。

## 1.1 产品侧现状与因果链（2026-09-15）

当前产品把默认 Codex App 作为用户拥有的进程处理：3R 只持久化写入可恢复的 Gateway 地址、准入头和模型目录投影，并通过独立兼容绑定让本地 Gateway 继续服务；不接管 App 的会话文件、`auth.json` 或 Electron 数据。Gateway（本机路由服务，负责接收 Codex 请求并按模型转发到上游）是否收到请求，决定了 502 的归因路径。

因果链如下：

1. 3R 安全启动默认 App 时，在这一次进程中注入 `NO_PROXY`（绕过系统代理的环境变量）并补齐 `127.0.0.1`、`localhost`、`::1`，所以回环请求可以到达 Gateway。
2. Finder / Dock 直开时只有磁盘上的配置投影，没有这次进程环境。系统代理若拦截回环，请求停在 Gateway 之前，表现为空正文 502，sidecar（Gateway 的后台进程）没有 `event=forward`。
3. 若 sidecar 有 `event=forward`，说明请求已进入 Gateway；此时 502 的责任域是上游连接、上游协议或上游对请求状态/历史的拒绝，不再是本机回环代理。

因此，产品当前对系统代理环境的支持路径仍是“从 3R 启动默认 App”，而“切换三方模型后旧会话无法继续”必须沿 Gateway 已转发的请求继续分析，不能用空 502 的回环结论覆盖。

## 2. 3R 启动默认 App 实际做了什么

### 写进磁盘（Finder 再开也还在）

`materialize_managed_app_config_for_target` 只改默认 home 的路由字段，带 marker，可停用恢复：

- 根级 `openai_base_url` → `http://127.0.0.1:43123/v1`
- `model_providers.OpenAI.base_url` → 同一 Gateway（Provider 级 URL 优先）
- `http_headers["X-3R-Instance-Key"]` 字面量（兼容路径不用 `env_http_headers`，避免 Finder 没有 3R 环境）
- `model_catalog_json` 指向生成目录
- 不改 `auth.json`、Electron 用户数据、会话

因此 Finder 打开的 App **一样会请求 43123**。能打到 Gateway 时，鉴权已经够。sidecar 在有活动兼容绑定时不会因关掉 3R 窗口立刻退出；3R 完全退出或机器重启后 sidecar 不在，那是网关没起来，和空 502 不是一类。

### 只加在这一次进程（Finder 没有）

`codex-launcher` 的 `configure_compat_app_command` 直接 spawn App 可执行文件（不是 `open -a`），并：

- 显式 `CODEX_HOME` / `CODEX_SQLITE_HOME` 指到默认 home
- `THREER_INSTANCE_KEY`（兼容模式主要仍靠 config 里的 header）
- 清掉 `OPENAI_API_KEY` / `CODEX_API_KEY` / `CODEX_ACCESS_TOKEN`
- `apply_loopback_no_proxy`：`NO_PROXY`/`no_proxy` = 父进程已有条目 + `127.0.0.1,localhost,::1`

最后一条才是系统代理下对话打通的原因。LaunchServices / Finder / Dock 拉起 `.app` 是干净的 Aqua 环境，不继承这次 spawn 的 env。Electron 单实例下，若 App 已由 Finder 打开，再从 3R 点一次往往只是聚焦旧进程，**不会补上 `NO_PROXY`**。

```text
3R 安全启动默认 App
  ├─ 磁盘：config.toml → 43123 + 实例头 + 目录     ← Finder 也有
  └─ 进程：NO_PROXY=…127.0.0.1,localhost,::1      ← Finder 没有
        → 不把回环交给 7892 → Gateway → 上游

Finder / Dock 直开
  ├─ 同一份 config.toml → 仍请求 127.0.0.1:43123
  └─ 无进程 NO_PROXY → 系统代理空 502 → sidecar 无 event=forward
```

代码：`src-tauri/src/bin/codex-launcher.rs`（`apply_loopback_no_proxy`、`configure_compat_app_command`）；`src-tauri/src/managed_codex_config.rs`（兼容字段物化）。

## 3. Codex 配置里没有「别走代理」

`ModelProviderInfo`（`deny_unknown_fields`）管的是打给模型 API 的那一跳，不是系统代理。

| 字段 / 提案 | 作用 | 对 Finder 空 502 |
| --- | --- | --- |
| `base_url` / `openai_base_url` | API 根地址 | 只决定 host；host 仍是 `127.0.0.1` 就会被拦 |
| `http_headers` / `env_http_headers` | 请求头 | 代理决定走不走 7892 **之后**才贴头。没有「请勿走代理」合同。往头里塞 `NO_PROXY` 无效，还可能转发出去 |
| `env_key`、`requires_openai_auth`、`wire_api`、重试 | 认证与协议 | 无关 |
| `[permissions.*.network] proxy_url` | 智能体沙箱出网 | 不是模型客户端 |
| `[shell_environment_policy] set` | 只影响 Codex 拉起的子命令 | 不影响 `/v1/responses` |
| `[features] respect_system_proxy` | 实验：更听话地用系统 PAC/代理 | Finder+Clash 会加重，不是绕过 |
| [issue #6060](https://github.com/openai/codex/issues/6060) `http_proxy` | 未合入。[PR #4261](https://github.com/openai/codex/pull/4261) 关闭。拟把值抄进 `HTTP_PROXY` | **强制走正向代理**，与回环绕过相反 |

出网代理今天认的是进程环境 `HTTP_PROXY` / `HTTPS_PROXY` / `NO_PROXY`（以及部分版本的 `$CODEX_HOME/.env`），**不从 `config.toml` 读 bypass 列表**。Desktop 走系统代理时，连 env `NO_PROXY` 也只有「进程里真有这个变量」才帮得上；Finder 默认没有。

## 4. 不采用的做法

| 做法 | 为何不做 |
| --- | --- |
| 换特殊域名（`*.local`、`*.localhost`、公网指回 127.0.0.1） | Gateway 只 `bind(127.0.0.1)`。没有解析就到不了；`localhost` 可能先走 `::1`（Gateway 不听）；Clash Fake-IP / mDNS 会把名字劫持走。例外规则匹配的是 URL host，不是「这是 3R」 |
| 3R 写 `~/.codex/.env` 的 `NO_PROXY` | Desktop 可能 dotenv，但 3R 不拥有该文件；和用户已有 `HTTP_PROXY` 搅在一起会搞挂官方出网（死掉的 `127.0.0.1:7890` 已有先例）。不是可恢复的 marker 字段 |
| `launchctl setenv` / `networksetup` 改系统 bypass | 改系统环境或网络，注销可能丢，和 VPN/Clash 抢写。违反「兼容模式不改系统代理」 |
| 包装一个带 env 的 .app 替换官方图标 | 劫持系统入口 |

用户自己改 Clash Skip / macOS「对这些主机和域不使用代理」可以修所有走系统代理的 App（含 Cockpit 类工具）。必须**分条**写 `127.0.0.1`、`localhost`、`::1`：打包成一个逗号串对不上 host；`127.*` 是 Clash glob，不是 Apple 语法。TUN/规则模式还要在核心里 `IP-CIDR,127.0.0.0/8,DIRECT`。这与 3R 无关。

换 Gateway 端口没用：被拦的是 host `127.0.0.1`。

## 5. 日后 Codex 支持配置级 `NO_PROXY` 时

彻底打通 Finder 的条件（需目标版本探针，不是 issue 标题）：

1. 用户级 `config.toml`（或 Desktop 在 LaunchServices 启动时也读的等价文件）有稳定 bypass 键，语义是 **NO_PROXY / 不走系统代理**，不是 `http_proxy`。
2. **Codex Desktop / Finder 直开**会用这份 bypass；不能只存在于 CLI 或 `$CODEX_HOME/.env`。
3. 能排除 `127.0.0.1`（建议同时 `localhost`、`::1`），且不把回环送进系统 HTTP 代理。
4. 未知键不会让整个 Provider 表解析失败（当前 `deny_unknown_fields`）。

满足后再把回环三项纳入兼容 marker 的可恢复字段。`http_proxy` 落地而没有 bypass 时，**不要**把 Gateway 地址配进正向代理。

未满足前，3R 文档和 UI 应把「从 3R 启动默认 App」当作系统代理环境下的支持路径；不要承诺 Dock/Finder 直开在 Clash 开启时可用。

默认环境卡片应把这条路径写成明确的操作提示：系统代理开启时，用户先完全退出 Finder/Dock 已打开的默认 App，再点击卡片中的 App 启动按钮。卡片中的模型配置按钮只负责写入默认 App 配置，不能替代带有 `NO_PROXY` 进程环境的 3R 启动。

## 6. 诊断时不要揉在一起

| 观察 | 含义 |
| --- | --- |
| 空 body 502，sidecar 无 `event=forward` | 系统代理吞了回环 |
| 有 `event=forward` 的 502 | Gateway 已收到，看上游 |
| 401 / 实例头 | 绑定或 header，不是代理 |
| 连接失败且 3R 已退出 | sidecar 不在 |
| App 里档位像 GPT | 目录克隆，见[第三方推理档位](third-party-model-reasoning-levels.md) |

不记录 API Key、实例密钥、请求正文、响应正文。sidecar 日志：`~/Library/Application Support/io.3r.codex/gateway-sidecar.log`。
