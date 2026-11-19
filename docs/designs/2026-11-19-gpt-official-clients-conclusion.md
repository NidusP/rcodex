# 诉求再审结论：GPT official clients only、探针与透出淡出

> 状态：active（Commander 已批注并对齐 2026-11-19；实现按 §2–5 验收）
>
> 本文是对你三条诉求的**再审结论文档**，不含实现承诺。审核通过后再改代码。  
> 取代/收敛：[2026-11-19-gpt-codex-identity-and-exposure.md](./2026-11-19-gpt-codex-identity-and-exposure.md)、[2026-11-19-codex-header-passthrough-channel.md](./2026-11-19-codex-header-passthrough-channel.md) 中已与下文冲突的表述。

---

## 0. 先前偏差（为何需要再审）

1. **把「完全透传」理解成「GPT 请求一律不注入渠道 API Key、原样转发 Authorization」**——与隔离实例「**多 Provider 合成目录 + 单一 API 登录 auth.json**」矛盾；也会与根合同「精确按 RouteSet 取渠道凭据」冲突（见下 §1.3）。
2. **把 cli_only 说成与模型族无关**——在你使用的**混合聚合**上，限制往往**在 GPT 路由/文本探针**上才出现；非 GPT 同 URL 可走 API Key 段且可用。
3. **在未审核方案前落了大块工作区 diff**（Gateway、`pass_through` 枚举、schema v25、探针/透出等）——部分与下述**目标合同**不一致，**不得视为已验收实现**。

---

## 1. 代码与产品现状（事实基线）

### 1.1 隔离 App 出站长什么样

| 项目 | 实际行为 |
| --- | --- |
| `model` | 目录中的**公共模型 id**（物化自启动时 RouteSet） |
| `Authorization` | 来自隔离 `auth.json` 的 **`OPENAI_API_KEY`**（工作区选定的**单一 API 登录渠道**） |
| 客户端指纹 | 真 Codex 出站：`User-Agent`、`Originator`、`x-codex-*` 等 |
| 回环目标 | `config` 指向 `127.0.0.1` Gateway + **实例准入密钥**（不发给上游） |

API 登录写入见 `managed_codex_config/isolated_auth.rs`：仅投影**一把** Key 到隔离 `auth.json`。

### 1.2 Gateway 今天对第三方路由做什么（`origin/main` 语义 + 工作区未提交前）

**数据路径（不变）**：`Codex → 127.0.0.1 Gateway（admit）→ 按 public model 精确匹配 RouteSet → 上游 Provider`。

| 步骤 | compat（非 GPT 或历史默认） | 工作区草稿对「GPT 族」的改动（**待你否决/采纳**） |
| --- | --- | --- |
| 准入 | 实例密钥 | 同左 |
| 请求体 | 非 GPT：relay 改写；GPT：`needs_relay_compat = false`，少改 body | 同左 + 草稿跳过 relay 预检分支 |
| `model` 字段 | 替换为 `upstream_model_id` | 同左（**必须保留**） |
| HTTP 身份头 | **不**转发；上游只见 Gateway 客户端 | 草稿：**转发**客户端头 |
| `Authorization` | **`forward_provider(..., None)`** → **只用**该路由 Provider revision 的 **渠道 API Key**；**丢弃** Codex 入站 Bearer | 草稿：**不注入**渠道 Key，**转发**入站 Bearer |

与历史产品文案的冲突：独立工作区计划曾写「**无认证头透传**」（指不对第三方透传**官方**订阅 OAuth）。你当前诉求是在 **GPT + cli_only 聚合段** 上例外：**透传客户端指纹**，且**不要用渠道 Key 冒充官方 Codex**。这需要在文档与 AGENTS 跨端合同中写清**例外边界**，而不是全盘「认证头透传」。

### 1.3 为何「路由仍要换模型 / 往往还要换 Key」

- **换模型**：公共 id 与上游 id 可能不同；渠道之间命名也不同。**无争议，必须保留。**
- **换 Key（或等价校正）**：同一隔离进程只有 **loginProviderId 对应的一把 Key** 在 `auth.json`，但 RouteSet 可含**多个 Provider** 的模型。Compat 路径用 **命中的 Provider revision 的 Key** 打上游，避免「Codex 仍带 A 渠道 Key 却打到 B 渠道 URL」。
- 因此：**「完全透传」不能等同于「Authorization 永远不换」**；你提出的质疑成立。

### 1.4 official clients only 在你场景里是什么

- **现象**：渠道**文本探针**优先试 **GPT 模型**，用 **探针 HTTP 客户端 + 渠道 API Key**（非 Codex）→ 上游 403 / official clients only。
- **含义**：该聚合上 **GPT 线路** 的验收合同是 **Codex 客户端身份**（头 + 常伴随特定会话形态），**不是**「任意客户端 + 渠道 Key」。
- **非 GPT**：同 base URL 下可走 **开放平台段**，探针与隔离 App 用 Key 仍可能 **OK**——故限制**表现为 GPT 相关**，不是「整站 OAuth 池与模型族无关」的简单图。

### 1.5 透出 / 探针 / 目录（现状）

- 非 GPT 透出仍须 `text_check = ready`（`read_probe_context`）。**GPT 族**经 Gateway 直传，透出与探针上下文仅要求渠道已配置，**不**阻塞于协议探针；**不**触发首次图片探针，模型目录**不展示**图片检测状态与刷新（默认具备看图能力）。
- 手动**取消透出**若空间仍引用 → `model_exposure_in_use`（[2026-11-07 模型透出](2026-11-07-model-exposure-image-probe.md)）。
- 渠道目录刷新成功写入新 revision 后，**原合同未要求**自动取消已透出但已下架的模型。

---

## 2. 对你三条诉求的**收窄后合同**（建议审核稿）

### 诉求 1：GPT 系列避免 official clients only

**目标**：隔离 App 请求经 Gateway 打聚合 **GPT 线路** 时，上游看到的是 **Codex 客户端合同**，而不是「被剥头 + 渠道 Key 冒充」。

**建议拆成两层（避免再混淆）：**

| 层 | GPT 路由应做 | 非 GPT 路由 |
| --- | --- | --- |
| **A. 客户端指纹** | 透传 `User-Agent`、`Originator`、`x-codex-*` 等（剥离 hop-by-hop / 实例准入头） | 不伪造 Codex 身份 |
| **B. Authorization** | **RouteSet 渠道 API Key**（不转发入站 Bearer） | 继续 **RouteSet 绑定渠道的 API Key** |
| **C. 请求体** | 保持现有 GPT Responses **少改写** | 现有 relay |
| **D. model** | **继续** public → upstream 替换 | 同左 |

**Authorization（Commander 批注，已锁定）**

- **不做 G1/G2 分岔**：`official clients only` 仅描述**探针**在无 Codex 指纹时的失败，不驱动运行时改 Auth 策略。
- **运行时统一**：GPT 族（及可选整渠道 `pass_through`）**透传客户端指纹头**，**不转发**入站 `Authorization`；上游 **`Authorization` = 命中的 Provider revision API Key**（与 compat 精确路由一致）。
- **用词**：**指纹透传 + RouteSet 渠道 Key**；不是「GPT 永不注入渠道 Key」。

**非目标（保持）**：不打开官方订阅 `RouteSource::Official` 发布门。

---

### 诉求 2：GPT 探针 official clients only → 仍可透出 + 提示 + 不看图探针

| 项 | 合同 |
| --- | --- |
| 目录 | `/models` 成功 → **照常入库** |
| 文本探针 | 仅 **GPT 候选** 因 **official clients only / cli_only** 失败 → `text_check = failed`，`error_code = gpt_codex_client_required`；**不**把渠道标成不可用到无法保存/透出 |
| 连通性总态 | 建议 **Ready 或 Degraded**（能列出 GPT），**不阻止** 添加渠道；`gpt_codex_client_required` 时报告仍写入推断的 `wire_api`（文本探针未过，仅供参考） |
| 隔离工作区 | **添加渠道**：已透出对话模型且（协议已确认 **或** 至少透出 GPT 族）；**勾选模型**：非 GPT 仍须协议已确认 |
| 图片探针 | **GPT 全族**：首次透出 **不排队** `should_probe`；已透出 GPT **`image_supported` 默认 true**（Commander 接受对 2026-11-07 的**书面例外**）；**非 GPT** 仍须 image-v1 成功证据 |
| 文案 | 明确：**参考探针，请启动隔离 App 自试**；错误码映射 `gpt_codex_client_required` |

**与诉求 1 的链接**：探针失败只说明 **探针不具备 Codex 指纹**；真请求依赖隔离 App 出站 + Gateway **指纹 + 渠道 Key**。

**`gpt_codex_client_required` 时 `/v1` 前缀（与 [探针地址阶梯](../archive/designs/2026-10-12-probe-url-ladder.md) 一致）**：文本探针仍按阶梯 `P/responses` → `P/v1/responses` → Chat… 依次 POST；**只有 HTTP 2xx 才停住并写入修订 base**。走兜底时（全部 GPT POST 均为 `upstream_cli_rejected` / `gpt_codex_client_required`）：取 **`calls` 中第一条 POST 的路由**，经 `confirmed_base_for_route(P, route)` 推断 `confirmed_base_url`（`/v1/responses` → `P/v1`，`/responses` → `P`）；`wire_api` 取第一条含 `responses` / `chat` 的失败路由。刷新渠道且实例未锁修订时，可把 `confirmed_base_url` 写回修订。Gateway 转发只用 **修订 base + wire**，不在运行时再猜 `/v1`。

**启动前 catalog/profile CLI 探针**：你已要求 **仅参考、不阻止启动**——与诉求 2 并列，保持。

---

### 诉求 3：目录刷新后下架模型 → 自动取消透出并清空间引用

| 项 | 合同 |
| --- | --- |
| 触发 | `save_model_catalog` 成功提交后，对比**最新 ready revision** 条目 |
| 动作 | 对已透出但 **不在新目录** 的 `upstream_model_id`：`exposed = 0`；**删除**各空间 `launch_preset_model_selections` 中对应 route identity |
| 与用户手动取消区别 | **仅系统自动淡出** 可绕过 `model_exposure_in_use`；**用户点击取消透出** 仍先检查空间引用 |
| 运行中实例 | 不 retro 改 RouteSet；**下次启动**生效（与现有「配置变更后重启」一致） |

---

## 3. 实现状态（2026-11-19，与工作区代码对齐）

| 模块 | 状态 | 锚点 |
| --- | --- | --- |
| Gateway GPT 指纹 + RouteSet Key | 已按 §5 锁定 | `codex_passthrough.rs`、`forward.rs` |
| 渠道探针 `gpt_codex_client_required` + 保留推断 `wire_api` | 已落地 | `channel_probe.rs` |
| 透出 / 图探针 UI 对 GPT 例外 | 已落地 | `model_exposure/store.rs`、前端透出列表 |
| 工作区添加渠道 + 启动 `provider_workspace_admissible` | 已落地 | `preset_validation.rs`、`prepare.rs` |
| 目录淡出 `fade_exposures_missing_from_catalog` | 已落地 | `model_exposure/store.rs` |
| 启动 catalog/profile 探针 | 参考不拦启动 | `prepare.rs`、`probe.rs` |
| `TransportMode::pass_through` + schema v25 | 已落地（高级/整渠道，非默认路径） | `migrate_v25.rs`、`TransportMode` |
| 历史草案 | 只读 | [gpt-codex-identity](./2026-11-19-gpt-codex-identity-and-exposure.md)、[header-passthrough](./2026-11-19-codex-header-passthrough-channel.md) 已标 historical |

---

## 4. 验收清单（Commander 桌面联调）

1. **GPT 路由**：多 Provider 空间选 Provider B 的 GPT 模型 → 上游 URL 为 B，`Authorization` 为 **B 的 revision Key**；指纹头存在（§5：不转发入站 Bearer）。
2. ~~G1 入站 Bearer~~：**已否决**（§5），不作为验收项。
3. 渠道探针：models 有 GPT、text 仅 cli_only 失败 → 可透出 GPT，无图探针任务，提示文案可见。
4. 目录刷新掉模型 → 透出 off + 空间选择移除；手动取消仍受 `model_exposure_in_use` 保护。
5. 回归：非 GPT compat、Grok agent_message relay、实例准入、日志脱敏不重开。

---

## 5. Commander 批注（2026-11-19）

1. **Authorization**：否决 G1；**统一**指纹透传 + RouteSet 渠道 Key 注入。
2. **GPT 默认看图**：接受；非 GPT 仍须探测。
3. 原「G1 仍可能 403」免责声明：**不采用**（随 G1 否决作废）。
4. **实现策略**：保留探针 / 透出 / 目录淡出 / 启动参考探针；**重写** Gateway Auth 与指纹逻辑（不合并「GPT 不注入 Key」草稿）。

---

## 6. 参考代码锚点（便于你或后续实现对读）

- 隔离 auth 投影：`src-tauri/src/managed_codex_config/isolated_auth.rs`
- Gateway 准入与路由：`src-tauri/src/managed_gateway/http.rs`
- 上游 Key 注入：`src-tauri/src/managed_gateway/forward.rs`（`apply_upstream_auth`）
- GPT body 少改写：`src-tauri/src/managed_gateway/responses_compat.rs`（`needs_relay_compat`）
- 渠道文本探针：`src-tauri/src/channel_probe.rs`
- 透出 / 图探针：`src-tauri/src/model_exposure/store.rs`
- 历史合同：根 [AGENTS.md](../../AGENTS.md) Gateway 节；[2026-09-21 独立工作区](./2026-09-21-isolated-only-product-plan.md) §2、§3

---

**下一步**：Gateway 单测与真机联调（隔离 App + cli_only 聚合）；更新 `CURRENT-STATE` 交接。