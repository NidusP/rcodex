# 隔离启动、审批与 Gateway 状态

> 状态：2026-10-10 简化方案已写入代码。标题栏不再显示 Gateway。应用启动就拉起 Gateway。真正退出先停实例再停 Gateway。5 秒没有回执不拆绑定。失联时标题栏只留「重启」。下面 2026-10-09 的条目保留为当时的核对记录。

## 2026-10-10 已落地

一个 `codex-gateway` 服务所有隔离空间。每个正在运行的隔离 CLI 或 App 各登记一条绑定，带自己的路由快照和准入密钥。

- 控制面 `setup` 调用 `ensure_serving()`，先在 `127.0.0.1:43123` 等待。端口被失联 sidecar 占用时不卡住窗口。
- 窗口使用系统标题栏，不再自绘标题栏。没有为请求数另开轮询。失联时，页面里的「R Codex」右侧显示「重启」，活动实例不禁用它。第一次读到失联会提示一次。
- `launch_unconfirmed` 不调用 `cleanup_managed_instance`。界面会刷新，让仍处于 `launching` 的实例进入现有每 2 秒核对；回执到达后改成 `running`，不补绑。
- 关窗口只隐藏控制面。菜单栏「退出」、程序坞「退出」和 Cmd+Q 都走 `exit(0)`，会停止 `launching`、`running`、`stopping`、`orphaned`、`degraded` 实例，再 `stop_if_unused()`。停止失败则阻止退出，并通过 `app-exit-blocked` 说明原因。
- 只有 Tauri 的重启退出码 `RESTART_EXIT_CODE` 保留实例和 sidecar。开发热更新是从外部结束控制面进程，不会走到这条退出回调，sidecar 因脱离进程会话而留下。
- 不迁移三枚旧配置，不改阻塞 Apple Event，不做发现进程还在就重新绑定的轮询。

回滚：恢复标题栏状态、退出时「有实例就阻止退出」，以及 `launch_unconfirmed` 时拆绑定。Gateway 改回第一次启动实例时才拉起。

## 目标

把今晚查清的三件事分开：哪些已经改完，哪些只是确认了现有行为，哪些要等决定后再改。

## 已落地

隔离 Profile 不再写入 `approvals_reviewer`。合并进 `config.toml` 时删除旧的 `auto_review`。模型目录里的 `auto_review_model_override` 保留，只在 Codex 真正进入自动审核时使用。

原因：Codex `0.162.0-alpha.17.2` 把 `approvals_reviewer = "auto_review"` 当成自动审核，沙箱提权不再询问。2026-10-10 实测，只删掉这个键也不够：终端界面在 `approval_policy` 缺省时使用 `granular`，并且 `sandbox_approval = false`，提权直接拒绝。隔离 Profile、合并后的 `config.toml` 和 CLI 启动参数因此显式写成 `on-request`。不为各 Codex 版本做配置写法适配器：`on-request` 是旧有询问值，0.162 仍接受。以后若某个版本删掉这个值或启动参数，再单独处理。

已改文件：

- `src-tauri/src/managed_codex_config/materialize.rs`
- `src-tauri/src/bin/codex-launcher.rs`
- `src-tauri/src/managed_launch/prepare.rs`
- `src-tauri/src/managed_launch/tests_prepare.rs`
- `src-tauri/src/managed_codex_config/tests_catalog.rs`
- 产品原理、混合路由设计、第三方目录说明、隔离 CLI 偏好继承说明

验证：`cargo test --lib relaunch_refreshes_owned_config_and_keeps_isolated_preferences` 与 `cargo test --lib tests_prepare` 通过。`cargo fmt --all -- --check` 通过。

2026-10-10：新开的隔离 CLI 已对沙箱外的 git 初始化弹出询问，用户确认行为正常。已经打开的旧会话不会变。

回滚：恢复 Profile 写入 `approvals_reviewer = "auto_review"`，并停止在合并时删除该键。

## 已核对、暂不改

### 三枚被忽略的旧配置

隔离 `config.toml` 里的 `disable_response_storage`、`network_access = "enabled"`、`windows_wsl_setup_acknowledged` 是从默认 `~/.codex/config.toml` 播种来的。当前 Codex CLI 不认识它们。直接打开默认 CLI 也会警告。

`disable_response_storage` 和 `windows_wsl_setup_acknowledged` 在这版二进制里已经不存在，忽略后没有行为变化。`network_access = "enabled"` 不会打开外网。现行写法是：

```toml
[sandbox_workspace_write]
network_access = true
```

因此只读 git（`status`、`diff`、`log`、`show`）可以跑。写 `.git` 的命令和 `fetch`、`pull`、`push` 仍然先被工作区沙箱挡住：`.git` 只读，外网也没有被这行旧配置打开。下次启动隔离 CLI 后，这些越界操作改为询问，用户批准后才能执行；不会预先放开网络或完整磁盘。

### 回执

回执是 `codex-launcher` 写的 `status.json`，不是 CLI 或 App 交回的消息。运行回执包含启动器进程号、Codex 进程号和 macOS 进程启动时间。退出回执复用同一文件并带上退出码。没有「用户拒绝授权」这种回执。

### 实例准入密钥

同一隔离空间里的 CLI 和 App 不共用一把请求密钥。每次启动都新生成一把实例准入密钥。CLI 同时使用新的实例编号。App 重复使用该空间的实例记录，但再次启动仍换新密钥。渠道 API Key 才是这一空间共用的。

### `inFlightRequests`

这是 Gateway 里正在转发的请求数，不是实例数，也不是打开的对话数。请求开始转发或做摘要时加 1，响应结束或调用方关掉流时减 1。2026-10-10 起界面不再用它区分空闲和转发。

对话过程中经常没有上游请求：本地工具、回合之间、等下一句都是空闲。另外标题栏不会在实例一直 `running` 时刷新这个数，所以一次空档读到的「空闲中」会留在界面上。计数逻辑本身没有算错。

## 待决定

### 1. 终端授权超过 5 秒

现状：`open -a iTerm` 成功只表示系统收下了打开请求。`await_launch_confirmation` 每 100 毫秒看一次，最多 5 秒。没有回执就返回 `launch_unconfirmed`，并调用 `cleanup_managed_instance` 拆掉 Gateway 绑定。实例记录仍停在 `launching`。

用户稍后点允许时，启动器仍会写回执。控制面每 2 秒核对活动实例，可以把这条记录改成 `running`。回执不会把绑定加回去。若当时没有别的绑定，Gateway 进程也会停掉。

用户点拒绝时，如果 `open` 已经返回成功，不会再有失败回执。只有 `open` 自己非零退出才能马上知道，日志是 `terminal_open_failed`。

已否决：改成阻塞的 Apple Event。要写两套终端脚本，处理取消和权限归属，iTerm 自己的首次设置框仍然收不到。成本不值得。

建议：5 秒没有回执时不要自动拆绑定。在 R Codex 里询问「终端授权还没完成，要现在拆掉 Gateway 吗」。选择拆掉才走现有清理。选择留下，则保持 `launching` 和绑定；回执到了就变成运行中。用户之后点「停止实例」同样拆绑定。

不要为了等授权把整个 R Codex 的轮询一直开着。现有核对只在有活动实例时每 2 秒跑一次，没有活动实例就停。这条未确认的实例已经处于 `launching`，就在这个循环里。

### 2. 绑定已被拆掉、稍后实例却在运行

如果仍保持今晚的自动拆绑定，轮询可以补救，但现在没有做。正在跑的 CLI 里已经有准入密钥。数据库撤销绑定时只把状态改成 `revoked`，哈希、路由快照和渠道密钥都还在。可以用原来的哈希重新拉起 Gateway 并登记，不必换新密钥。

限制：用户点了「停止实例」或「删除实例记录」之后，不能因为进程还在就自动绑回去。停止是通知该 Codex 退出并故意拆绑定。删除只针对已经退出或启动失败的记录。

若采用第 1 项的询问并且用户选择留下，就不需要这条补绑。补绑只覆盖「已经拆了、用户后来又允许 CLI 起来」的旧行为。

### 3. 标题栏空闲 / 转发

Gateway 处于服务中时，按一个短间隔读取 `gateway_status`，用 `inFlightRequests` 更新标题栏。Gateway 停止后不再读。这只修正显示，不恢复被拆掉的绑定。

### 4. 外网配置

是否在播种或合并时把旧的 `network_access = "enabled"` 写成 `[sandbox_workspace_write] network_access = true`，并丢掉另外两枚已失效的键。不做的话，询问提权之后，外网仍要等用户批准，而不是随配置打开。

## 2026-10-10 决定

- 第 1 项不做。5 秒未确认仍拆绑定，不在 R Codex 里询问。
- 第 4 项不做。三枚旧配置警告留着。询问提权已经由去掉 `approvals_reviewer` 覆盖，外网不预先打开。
- 第 2 项和第 3 项要做，但不是两套轮询。用户点击启动隔离 CLI 或 App 后，只要这条实例还在活动状态里，就沿用现有的每 2 秒核对。每一轮做两件事：读 `gateway_status` 刷新标题栏的空闲或转发；若实例已是 `running` 且绑定只是被这次未确认拆掉，用原来的哈希、路由快照和渠道密钥重新登记 Gateway。
- 停止实例和删除实例记录不补绑。没有活动实例就停止这轮核对，R Codex 空闲时不轮询。
- `launch_unconfirmed` 现在直接进错误提示，不刷新实例列表。不刷新的话界面不知道这条 `launching` 实例，轮询不会开始。实现时失败路径也要把实例放进活动列表。

`inFlightRequests` 只在 Gateway 进程里实时加减。标题栏拿到的是上一次 `gateway_status` 调用的副本，没有推送。实例一直 `running` 时现有循环不读这个数，所以界面不会跟着变。

## 建议顺序

1. 手动重启一次隔离 CLI，确认不再出现 `approvals_reviewer`，提权会询问。
2. 扩展现有活动实例核对：失败启动也进入列表；每轮补绑并刷新标题栏请求数。

## 非目标

- 不改阻塞 Apple Event。
- 不把同一隔离空间的 CLI 和 App 改成共用一把准入密钥。
- 不把 `inFlightRequests` 改成对话数或实例数。
- 不在 R Codex 整个运行期间轮询。

## 改造计划（2026-10-10）

用应用生命周期代替标题栏状态和补绑轮询。第 1 项、第 4 项仍然不做。

### 目标

- 标题栏不再显示 Gateway 状态，也不再为「空闲中 / 转发中」轮询。
- R Codex 启动后立刻拉起 Gateway，此时可以还没有绑定，等隔离 CLI 或 App 启动时再登记。
- 用户真正退出 R Codex 时，先停止全部活动隔离实例，绑定注销后 Gateway 按现有 `stop_if_unused()` 停止。
- 开发热更新保持现在的例外：退出码表示受控重启时不停实例、不停 Gateway。已有活跃实例时继续复用 sidecar。

### 和现在的差别

现在 Gateway 在第一次启动隔离实例时才 `ensure_serving()`。关闭窗口只隐藏控制面。真正退出时如果还有 `launching`、`running`、`stopping`、`orphaned` 或 `degraded` 实例，会阻止退出。没有活动实例才 `stop_if_unused()`。

5 秒没有回执会 `launch_unconfirmed` 并拆掉绑定。Gateway 常驻之后，这次未确认不再拆绑定，也不再因此停止 Gateway。回执稍后到达，现有实例核对把记录改成 `running` 即可，不另做补绑轮询。

### 改动

1. 标题栏去掉 Gateway 状态点、空闲和转发文案。失联时保留「重启」。状态命令仍给诊断和这个按钮使用，不为请求数轮询。
2. 控制面 `setup` 完成后调用 `ensure_serving()`。失败时应用仍打开，并给出一次失败提示，不把窗口卡住。
3. 菜单栏「退出」、程序坞「退出」、Cmd+Q 和其他非重启退出：逐个停止活动隔离实例，再 `stop_if_unused()`。停止失败则仍阻止退出并说明原因。`exit(0)` 也算真正退出，不能因为带了退出码就跳过。
4. 只有 Tauri `RESTART_EXIT_CODE` 直接返回。开发热更新从外部结束控制面进程，不靠这个分支。
5. `launch_unconfirmed` 只报告「终端还没确认」，不调用 `cleanup_managed_instance`。用户之后点「停止实例」，或退出应用，才拆绑定。
6. 关窗口仍只隐藏，不当成退出。macOS 停靠栏图标可以消失，菜单栏入口留下。

### 不改

- 不写 `approvals_reviewer` 的改动保持。
- 不迁移三枚旧配置，不改阻塞 Apple Event。
- 不把 `inFlightRequests` 推到界面。
- 开发环境不在热更新时杀掉仍被实例使用的 Gateway。

### 验收

- 打开 R Codex 后，尚未启动实例时 `127.0.0.1:43123` 已在监听。
- 标题栏没有 Gateway 字样。
- 启动隔离 CLI 后授权超过 5 秒，绑定仍在；授权完成后请求能转发。
- 退出 R Codex 后，隔离 CLI 和 App 进程退出，Gateway 不再监听。
- `pnpm run tauri:dev` 热更新时，已运行的隔离实例和 Gateway 不断。

### 回滚

恢复标题栏状态、退出时「有实例就阻止退出」，以及 `launch_unconfirmed` 时拆绑定。Gateway 改回第一次启动实例时才拉起。

失联恢复已落地：控制令牌文件丢失且 `127.0.0.1:43123` 仍被旧 `codex-gateway` 占用时，标题栏右侧显示「重启」。点击关闭该进程并启动新 sidecar。见 [孤儿 Gateway](2026-10-09-orphan-gateway-sidecar.md)。
