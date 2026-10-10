# 丢失控制令牌后的孤儿 Gateway

## 控制面手动清理合同（2026-10-09）

当控制令牌文件丢失、但数据口仍被旧 sidecar 占用时，状态是 `unavailable`。标题栏右侧出现「重启」按钮，对应 `restart_unavailable_gateway`。它先沿用失联清理：只停止确认过的 `codex-gateway`，未知进程占用端口时拒绝。端口释放后立刻 `ensure_serving()`，新 sidecar 写入新的控制令牌，并从数据库恢复仍活动的绑定。正在进行的请求会中断。活动隔离实例不再禁用这个按钮。旧的 `cleanup_unavailable_gateway` 仍只清理并返回 `stopped`，且仍拒绝在有活动实例时执行。命令只接受 `unavailable` 状态；清理阶段在
`127.0.0.1:43123` 上枚举监听 PID，确认每个进程的可执行文件名为 R Codex 的
`codex-gateway` 后才发送停止信号。确认不是 R Codex Gateway 的占用进程时拒绝操作，
不得按端口直接杀掉未知进程。

清理是幂等的：旧 Gateway 已退出时返回 `stopped`；成功停止后删除残留
`gateway-sidecar.json`，清除控制面内存中的旧 endpoint/unavailable 标记，并允许下一次
启动重新创建 sidecar。标题栏按钮只在 `unavailable` 状态显示，执行期间禁用并通过通知报告
成功或失败。标题栏不再显示 Gateway 状态点。重启命令不因活动实例禁用；点击会中断仍连接旧端口的请求，并从数据库恢复活动绑定。只清理不重启的命令仍在有活动实例时拒绝。

控制面访问本机 `127.0.0.1` 伴随进程时明确绕过系统代理，避免代理返回非 JSON 内容而将健康的
本机 Gateway 误报为不可用。

非目标：不删除默认 Codex 配置、认证、会话或其他应用进程；不自动清理 serving Gateway。

Gateway 是独立的 `codex-gateway` 进程，不是隔离 CLI 的子进程。关掉 CLI 后，由 R Codex 控制面注销 binding，没有其他实例时再 `stop_if_unused()`。这需要 `gateway-sidecar.json` 里的控制令牌。

令牌文件被删掉后，控制面联系不上仍占用 `127.0.0.1:43123` 的 sidecar，会把「没有 endpoint」当成已停止。隔离 CLI 可以继续转发，标题栏显示「Gateway 不可用」。这时关 CLI 不会自动关掉旧网关。

## 先看会不会自动停

1. 停掉当前隔离 CLI（或隔离 App）。
2. 等几秒，让仍在运行的 R Codex 做实例核验。
3. 在**默认环境**（不要用隔离 CLI）检查数据口：

~~~bash
lsof -nP -iTCP:43123 -sTCP:LISTEN
~~~

没有 `codex-gateway` 监听，即可直接重启 R Codex，再启动新的 CLI。

## 仍在监听时，优先从标题栏清理

1. 标题栏右侧出现「重启」时即可点击，不必先停掉隔离实例。
2. 控制面只会停止确认过的 `codex-gateway` 监听进程；端口被其他程序占用时会拒绝，不会按端口误杀。
3. 成功后通知「已关闭失联的旧 Gateway，并启动了新的 sidecar」。新进程重新写入 ready 文件，并恢复数据库里仍活动的绑定。

## 仅当控制面无法打开时，手动关掉

同时确认 ready 文件是否还在：

~~~bash
ls -l "$HOME/Library/Application Support/io.rcodex/gateway-sidecar.json"
~~~

文件不存在，且 `lsof` 仍显示 `codex-gateway` 听着 `127.0.0.1:43123`，就是孤儿 sidecar。记下 PID 后：

~~~bash
kill <PID>
lsof -nP -iTCP:43123 -sTCP:LISTEN
~~~

还在监听再用 `kill -9 <PID>`。Windows 上对应结束 `codex-gateway.exe`，并确认 `127.0.0.1:43123` 已释放。

CLI 或 App 还连着 `43123` 时不要杀网关，当前对话会立刻断开。先停隔离实例，再杀 sidecar。

然后重启 R Codex，从控制面启动新的 CLI。新 sidecar 会重新写入 ready 文件并绑定 `43123`。

不要对 `codex-gateway` 发默认 Codex 的停止信号，也不要动用户原 Codex 目录。
