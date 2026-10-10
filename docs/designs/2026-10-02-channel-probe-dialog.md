# 添加模型渠道弹窗与探针结果

> 状态：已实现。2026-10-03。审核时删除了模型渠道编辑。
>
> 日期：2026-10-02。

## 审核结论

模型渠道不再提供编辑。界面不修改名称、Base URL、API Key 或 Gateway 超时。密钥失效或地址错误时，删除该渠道后重新添加。诊断和渠道行里的「编辑密钥」「编辑地址」去掉，仍可重新检测。

后端的 update_api_key_provider 先留给既有测试，产品界面不再调用。已有直连 Responses 渠道在探针仍确认 Responses 时，不改成 Gateway 透传。

## 目标

用户在添加模型渠道时看到居中弹窗，而不是右侧抽屉。名称、Base URL、API Key 都已填写后，可以点击「探针测试」。探测成功时，弹窗显示协议类型和可用模型。结果写入本机库，模型渠道列表里可以查看。之后点击「刷新模型目录」会用同一次探测更新这份结果。

探针指一次由用户点击触发的上游检查：读取模型列表，并在需要时发送最多两次最小文本请求，用来判断上游接受 Responses 还是 Chat Completions。它不是 Gateway（本机回环转发）在真实对话失败后的自动重试。

## 现状

添加和编辑共用 [src/App.tsx](../../src/App.tsx) 的 AddProviderDrawer。它是右侧 drawer-layer，字段为名称、Base URL、API Key，以及非直连时的 Gateway 超时。保存走 create_api_key_provider。API Key 只进系统凭据库。新建渠道默认 transport_mode = gateway_passthrough，修订上的 wire_api 因此是 responses。表单里没有协议选择。

已保存渠道的「刷新模型目录」调用 test_api_key_provider，再进入 connectivity::test_provider。它用已入库的 Key 做 GET {base}/models，再按当前 transport_mode 发一次最小文本请求。模型列表成功时写入 provider_model_catalog_*；连通状态写入 provider_connectivity_reports。报告没有协议字段。读取报告时，模型来自当前修订下最新一条 ready 目录。

模型渠道列表有状态徽章和「检测于」。完整模型名在「查看支持模型」抽屉。工作区已加入渠道行显示的是已选进工作区的模型，不是探测到的模型。

因此现在做不到三件事：未保存前不能探测；探测不会判断协议；列表看不到协议。

## 非目标

- 不把探针变成保存的前置条件。未探测也可以保存，列表显示「未检测」。
- 不在编辑弹窗放探针按钮。编辑不回显 API Key，页面不能拿旧 Key 做草稿探测。已保存渠道仍用「刷新模型目录」，那个动作从凭据库读 Key。
- 不改工作区里「添加渠道」菜单。那是把已有渠道加入工作区，不是新建渠道。
- 不在 Gateway 转发失败时换协议、换地址、换模型或重试 POST。
- 不按模型名字猜协议。
- 不把 API Key、请求正文或响应正文写入 SQLite、日志、诊断或界面状态。
- 不改运行中实例已经固化的 RouteSet。RouteSet 是一次启动时固化的路由快照。
- 不把探测结果写成 direct_responses。直连仍是旧字段，不是探针结论。
- 本轮不启动 pnpm run tauri:dev，也不打包。

## 请审核的决定

下面六条是实现边界。否掉任何一条都要先改本文，再写代码。

1. 只保留添加弹窗。编辑模型渠道的入口删除，不另做编辑弹窗。
2. 保存不要求探针成功。
3. 协议判定先试 Responses。只有上游明确表示这条路径不存在时，才再试一次 Chat Completions。401、403、429、5xx、超时和网络失败都不换协议。
4. 探测成功且渠道没有被运行实例锁定时，把协议写进渠道修订，下次启动按它转发。锁定期间只更新可查看的探针记录，不改修订。
5. 未点保存之前，探针结果只留在内存票据里，不入库。保存时 Base URL 和 API Key 仍与票据一致，才把结果写入该渠道。
6. 列表显示协议，以及最多 3 个可用模型名和总数。完整名单仍打开现有「查看支持模型」抽屉。

## 用户可见行为

### 弹窗

「添加模型渠道」和「编辑模型渠道」改为 dialog-layer 上的居中对话框，标题和字段保持现在的文案。宽度约 640px，内部滚动，不复用删除确认框那套窄布局。遮罩点击和 Escape 关闭；探测或保存进行中不关闭。

添加弹窗底部是「探针测试」「保存模型渠道」「取消」。探针按钮不是提交按钮。

按钮仅在去除首尾空白后的名称、Base URL、API Key 都非空，且当前没有探测或保存进行时可以点击。缺字段时用现有禁用提示：「请先填写名称、Base URL 和 API Key。」名称不参与上游请求，但按本次要求作为点击条件。URL 是否合法不在按钮上预判；点击后由现有 Base URL 校验拒绝，不发出请求。

探测进行时，输入和两个主动作都禁用，按钮显示「正在探测…」。

结果区：

- 尚未探测：说明这次会读取模型列表，并可能发送最小文本请求，上游可能计费。
- 成功：状态为成功；协议显示「Responses」或「Chat Completions」；其下是可用模型的显示名和上游 ID。Codex 系统角色模型继续用 visibleProviderModels 隐藏；N 大于 0 时说明「已隐藏 N 个 Codex 系统角色模型，它们用于自动审核或子代理等特殊调用，不会出现在 Codex 模型目录，也不能作为普通模型加入工作区。」 图像生成模型同样不列入可勾选对话模型；有这类模型时说明它们用于单独出图，不会显示在工作区勾选中。
- 失败：显示现有连通状态文案和错误码。不显示协议，也不把模型列表标成成功。
- 模型列表读到了，但协议没确认：状态不是成功。显示模型，协议位置写「未确认」。不把渠道改成某种协议。

Base URL 或 API Key 再改动时，清掉结果和票据。只改名称不清结果。关闭弹窗或保存完成时丢掉内存票据。保存进行时不能改字段。

编辑弹窗保留「API Key 留空则不更换密钥」。没有探针按钮。说明改为：改完后到列表里刷新模型目录。

### 列表

模型渠道列表每一行，在现有状态旁显示协议和可用模型摘要。有协议时写 Responses 或 Chat Completions；这次探测没确认时写「未确认」；从未探测写「未检测」。模型摘要为前 3 个显示名，多于 3 个时加 +N，并带总数。点击现有层级图标打开的目录抽屉在标题区加上协议和探测时间。抽屉列出这次探测到的全部模型，并写明每条是对话、图像生成还是 Codex 系统角色。对话模型可在工作区勾选；图像生成（例如 grok-imagine-image-2.0）和系统角色不会出现在工作区勾选中。

工作区已加入渠道行增加协议和「探测到 N 个模型」。原来的「已选择模型」一行保留，两套名字不要混用。加入渠道确认条在现有「已发现 N 个模型」旁加上协议。

### 刷新

「刷新模型目录」仍是显式动作，入口不变。它执行同一套协议判定，并覆盖该渠道的最新探针结果：状态、协议、模型目录、探测时间。协议没变时不新写渠道修订，只更新报告和目录。协议变了且渠道未锁定时，走现有修订写入，把 wire_api 和对应的 transport_mode 写成新值。已经启动的实例继续用旧 RouteSet，下次启动才用新协议。

渠道被运行实例锁定时，刷新仍更新报告和目录，让列表立刻能看到新结果，但不改修订。界面说明：这次探测到的协议已显示；正在运行的实例不变；实例停止后需要再刷新一次，下次启动才会改用新协议。不在解锁时自动补写修订，避免一次旧探测在用户无操作时生效。

刷新没读到模型列表时，更新报告为失败，不把上一份成功目录当成这次的结果。旧目录行可以留在库里，但当前报告不引用它们。

## 探针合同

添加弹窗调用新的草稿命令，参数是 Base URL 和 API Key。名称只在界面上作为按钮条件。命令先走现有 validate_base_url / normalize_api_base_url：必须是 HTTPS，只有回环地址可以用 HTTP；去掉查询和片段；空路径补成 /v1。

然后：

1. GET {base}/models，Bearer 为这次输入的 Key。解析仍只取 id、name、owned_by。
2. 列表不可读、为空，或认证失败：停止。不发文本请求。401/403 记为未授权。
3. 列表有模型时，用第一个模型 ID 做非流式最小文本请求。先 POST {base}/responses，正文与现在的 Responses 探针相同：一句文本，max_output_tokens 为 8，stream 为 false。
4. Responses 返回 2xx 且 JSON 可解析：协议为 responses。不再请求 Chat。
5. 只有路径明确不存在时才试 Chat 一次：HTTP 404 或 405，或 400/404 且错误体属于固定的「未知路径 / 路由不存在」集合。模型不存在、参数错误、认证失败都不算路径不存在。
6. Chat 使用现在的最小正文：POST {base}/chat/completions，max_tokens 为 8，stream 为 false。2xx 且 JSON 可解析则为 chat_completions。
7. 429、5xx、超时、连接失败：停在已尝试的那一次，协议留空。不因为 502 再打另一个路径。

超时沿用现在的 15 秒探针超时，不用表单里的 Gateway 超时。客户端不跟随重定向。一次点击最多 1 次 GET 加 2 次 POST；Responses 成功时只有 1 次 POST。

成功的定义是：模型列表检查为 ready，文本检查为 ready，且 wire_api 为 responses 或 chat_completions。只有这种情况才在未锁定时改渠道协议。

协议落到现有两个字段，避免再引入第三套开关：

| 探针协议 | wire_api | transport_mode |
| --- | --- | --- |
| Responses | responses | gateway_passthrough |
| Chat Completions | chat_completions | gateway_chat_completions |

未探测而保存的新渠道保持今天的默认值：gateway_passthrough / responses。

两种协议都返回成功时，这一轮固定采用 Responses。判定在 Responses 成功后不会再请求 Chat，所以不会同时看到两个成功结果。弹窗不提供手工改选。

## 写入与所有权

权威写入仍在后端。界面不能提交一份自己编的模型列表或协议让库接受。

草稿命令跑在 Tauri 主进程，不经过 Gateway sidecar。它把结果放进主进程内存票据，不写 SQLite，不写凭据库，不写诊断事件。票据包含随机 ID、规范化后的 Base URL、API Key 的 SHA-256、探测结果和创建时间。不保存 Key 原文。票据最多 8 张，存活 10 分钟，用掉或过期即删。进程重启后票据消失；用户需要重新探测。重启前弹窗本身也会关掉。

create_api_key_provider 增加可选 probeTicketId。保存时：

1. 仍按现有事务创建渠道和凭据。
2. 没有票据：行为与现在相同。
3. 票据的 URL 和 Key 指纹与这次提交一致：在创建得到 provider_id 和修订 ID 之后，写入连通报告、模型目录，并让第一条修订使用探测到的协议。诊断事件这时才记，provider_id 用新渠道，内容仍只有路径、状态、耗时和错误码。
4. 票据不匹配或过期：渠道仍然保存，但不附带探针结果。返回里标明没附上，界面告诉用户地址或密钥已变化，结果没有保存。

票据写入失败不回滚已经创建的渠道和凭据。界面提示渠道已保存、结果没写入，并让用户使用刷新。刷新会再访问上游。

刷新命令继续是 test_api_key_provider。它从凭据库取 Key，调用同一判定函数，然后更新报告和目录。只有协议变化且渠道未被运行实例锁定时才新写修订。判定函数是唯一的协议决策点；草稿命令和刷新命令都调用它，不能各写一套分支。

模型目录仍由 DiagnosticsStore::save_model_catalog 写入。报告通过新的 catalog_revision_id 指向这次探测写下的目录。这是写入时校验的逻辑引用，不是 ALTER TABLE 加上的外键；升级不重建表。读取时优先用这个 ID。旧报告该列为空，继续读当前修订下最新的 ready 目录，避免升级后已有渠道突然变成没有模型。

## 库表

当前 schema 用户版本是 18。本变更升到 19，只做加法。

provider_connectivity_reports 增加两列，都允许空：

- wire_api。非空时只能是 responses 或 chat_completions，由 Rust 写入前检查。
- catalog_revision_id。指向某次 provider_model_catalog_revisions.id。

不把模型数组塞进报告行。模型的权威清单仍是目录表。

前端 ProviderConnectivityReport 增加 wireApi，取值为 responses、chat_completions 或空。草稿命令的返回另外带 probeTicketId，只有添加弹窗持有，不进渠道列表状态。

## 失败与隐私

| 情况 | 行为 |
| --- | --- |
| 名称为空、URL 为空或 Key 为空 | 按钮不可点 |
| URL 非法 | 不访问上游，弹窗显示现有 URL 错误 |
| 401/403 | 停止，状态未授权，不试另一种协议 |
| 429、5xx、超时、断网 | 停止，状态按现有分类，不试另一种协议 |
| /models 成功但协议未确认 | 可保存模型和「未确认」，不改修订协议 |
| 探测成功后改了 URL 或 Key | 界面丢弃结果；若仍提交旧票据，保存渠道但不附带结果 |
| 保存时票据过期 | 渠道保存，列表为未检测 |
| 渠道被实例锁定 | 可刷新报告和目录，不改修订 |
| 关闭弹窗 | 丢弃未保存票据，不留 Key |

诊断继续只记形状：来源 probe、路径、状态、耗时、错误码。不记 Authorization、Key、请求体、响应体、带查询的 URL。

## 实现切片

审核前不开始。通过后按波次做。同一波次的文件不重叠；后一波才碰共享文件。

### 第 1 波，可同时做

**A. 报告字段和迁移**

- 负责：src-tauri/src/database.rs、src-tauri/src/diagnostics.rs
- 依赖：无
- 做：schema 19；报告读写两个新列；旧行兼容；迁移测试
- 不做：不改 HTTP 探测
- 验证：cargo test 中 database 迁移与 diagnostics 报告往返

**B. 协议判定**

- 负责：新建 src-tauri/src/channel_probe.rs
- 依赖：无。判定结果用本文件里的纯结构，不提前改 ProviderConnectivityReport
- 做：上文的 GET/POST 顺序、停止条件和 mock 测试
- 不做：不注册 Tauri 命令，不写库
- 验证：cargo test --lib channel_probe

**C. 弹窗组件**

- 负责：新建 src/provider-channel-dialog.tsx；样式写在对应组件 CSS，不改全局主题文件
- 依赖：无。先按本文的返回形状写 props，不挂到 App.tsx
- 做：居中弹窗、按钮启用条件、结果区、字段变化后丢弃结果
- 不做：不删 AddProviderDrawer，不接真实命令
- 验证：新文件自身类型闭合。挂载后的 pnpm run check 放在第 3 波

### 第 2 波，A 和 B 都完成后同时做

**D. 接到保存和刷新**

- 负责：src-tauri/src/connectivity.rs、src-tauri/src/providers.rs、src-tauri/src/lib.rs
- 依赖：A 的报告字段，B 的判定函数
- 做：草稿命令和内存票据；创建渠道时核销票据；刷新改为调用判定函数；未锁定且协议变化时写新修订；锁定时拒绝改修订
- 不做：不改界面
- 验证：草稿命令不写渠道和报告；票据匹配才写入；URL 或 Key 变化不附带结果；401 不发生第二次 POST；502 不换协议；锁定渠道的修订 ID 不变；未锁定且协议变化时出现新修订

**E. 列表展示**

- 负责：src/workspace-surface.tsx、src/channel-sort-list.tsx，以及 App.tsx 里渠道行映射、ProviderList、模型目录抽屉
- 依赖：A 的 wireApi 字段已经出现在报告类型里。D 未完成时可以用空值编译
- 做：协议、最多 3 个模型名、目录抽屉标题、工作区行上的「探测到 N 个」
- 不做：不改添加弹窗，不改 AddProviderDrawer 的调用点
- 验证：类型检查纳入第 3 波

E 和 D 可以并行，是因为 E 只消费报告字段，D 不改这几个界面文件。若 D 发现报告字段不够，停下来改 A 的合同，不要在 E 的文件里临时加第二份数据。

### 第 3 波，串行集成

**F. 挂上弹窗并收口**

- 负责：App.tsx 中替换 AddProviderDrawer 的那一处，以及 C 的组件接线
- 依赖：C、D、E
- 做：添加用新弹窗并传入草稿命令；编辑用同一弹窗但不传探针；保存携带票据 ID；删除旧抽屉组件
- 验证：pnpm run check；cargo test --lib channel_probe；cargo test --lib connectivity；cargo test --lib diagnostics；database 迁移相关用例；cargo fmt --all -- --check
- 不跑：pnpm run tauri:dev、pnpm run tauri:build、codesign

手动验收留给 commander 开着开发窗口时做：填齐三项才能点探针；成功看到协议和模型；保存后列表出现同一结果；改 Key 后旧结果不入库；刷新后列表变化；锁定时列表更新但运行实例的路由修订不变。

## 文档同步

实现时只补三处，不新开计划：

- 本文件状态改为已实现，并写上实际命令和结果。
- docs/codex-hybrid-routing-design.md 里「模型目录刷新是显式动作」一句加上：刷新同时更新探针协议；运行中的 RouteSet 不改。
- docs/codex-roadmap-and-gateway-diagnostics.md 的探测小节加上协议判定的停止条件。

## 回滚

界面回退到 AddProviderDrawer 即可恢复抽屉。schema 19 的两列可空，旧插入语句按列名写入，不依赖它们。已经写成 Chat 的渠道修订不会自动改回 Responses；若要恢复某个渠道，用编辑保存或再刷新，不提供批量改写。

## 未决但先不展开

编辑时若将来允许「用已存 Key 再探测」，必须由后端读凭据库，不能把 Key 送回界面。那不是这一轮。


## 实现记录

- 添加渠道改为居中弹窗，编辑入口已删除。
- 探针命令是 probe_channel_draft。结果先放主进程内存票据，保存时核销。
- 刷新模型目录走同一套协议判定。渠道被运行实例锁定时只更新报告和目录。
- 验证：cargo test --lib 中 channel_probe、connectivity、diagnostics 共 14 项通过；pnpm run check 通过。未运行 tauri:dev。
