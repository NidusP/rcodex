# 开发版与正式包身份分离

> 状态：已落地。正式包保持 `io.rcodex` 和 Gateway `43123`。只有 `tauri dev` 使用 `io.rcodex.dev` 和 `43124`。

## 问题

`tauri dev` 和已经构建的 R Codex.app 使用同一个 `identifier` `io.rcodex`。单实例插件按这个身份判断，后打开的一方不会新开窗口，只会把已有窗口叫到前台。两边还共用 `127.0.0.1:43123` 和同一份应用数据目录，所以会抢 Gateway，也会写同一套渠道和会话库。

## 目标

开发版和正式包可以同时打开。Dock 和窗口标题能分清。各自使用自己的 Gateway 端口和数据目录。正式包的身份、已安装用户的数据和在线更新不变。

## 非目标

- 不改正式包的 `io.rcodex`、`R Codex`、43123、更新公钥和发版仓库。
- 不把开发版数据复制或迁移到正式包，也不反向复制。
- 不改变隔离 CLI 在 Windows 上仍然拒绝启动的现状。
- 不做 Apple 公证。

## 做法

1. 新增 `src-tauri/tauri.dev.conf.json`，只覆盖：
   - `identifier`: `io.rcodex.dev`
   - `productName`: `R Codex Dev`
2. `scripts/tauri.mjs` 仅在命令是 `dev` 时追加 `--config src-tauri/tauri.dev.conf.json`。`build` 不读这份覆盖。配置合并如果会整体替换 `app.windows` 数组，覆盖文件里不要写 `windows`，避免丢掉现有窗口尺寸。
3. 窗口标题不放进覆盖文件。应用启动时，debug 构建把主窗口标题设为「R Codex Dev」。这只发生在真正的开发进程里，单元测试不走这条路径。
4. Gateway 默认端口仍是常量 `43123`。开发脚本给 `tauri dev` 进程设置 `RCODEX_GATEWAY_PORT=43124`，控制面用这个值启动和探测 sidecar，sidecar 继续从命令行参数接收端口。未设置环境变量时保持 43123，因此 `cargo test` 和正式包都不变。
5. 数据目录继续由 Tauri 按 `identifier` 决定。开发版第一次打开是空库。
   - macOS：正式包 `~/Library/Application Support/io.rcodex`，开发版 `~/Library/Application Support/io.rcodex.dev`。
   - Windows：正式包 `%APPDATA%\io.rcodex`，开发版 `%APPDATA%\io.rcodex.dev`。也就是 `C:\Users\<用户>\AppData\Roaming\` 下面的两个目录。
6. 单实例插件不改代码。macOS 和 Windows 都按 `identifier` 区分，身份不同之后两边不再互相唤醒。端口同样是正式包 43123、开发版 43124。

隔离实例在启动时记下当时的 Gateway 地址。开发版拉起的实例只连 43124，正式包拉起的只连 43123，不会串到对方的渠道。

## 验收

- `tauri dev` 已打开时，再打开正式包，出现第二个窗口，而不是只把开发窗口叫到前台。
- macOS Dock 或 Windows 任务栏中，一个是 R Codex，一个是 R Codex Dev。
- 开发版监听 43124，正式包监听 43123。
- 开发版的渠道库是空的，正式包原有渠道还在。
- `cargo test` 里对 `GATEWAY_PORT` 的断言仍然是 43123。
- `tauri build` 产物的 identifier 仍然是 `io.rcodex`。

`tauri dev` 会把 `TAURI_CONFIG={"productName":"R Codex Dev","identifier":"io.rcodex.dev"}` 留在进程环境里。生产构建必须删掉这个变量再编译，否则 `Info.plist` 仍是 `io.rcodex`，二进制运行时身份却是 `io.rcodex.dev`，一点击就会唤醒正在跑的 Dev。`scripts/tauri.mjs` 在 `build` 时会删除 `TAURI_CONFIG` 和 `RCODEX_GATEWAY_PORT`。

## 回滚

去掉 dev 配置覆盖、开发脚本里的 `--config` 和环境变量，并去掉窗口标题的 debug 分支。已产生的 `io.rcodex.dev` 目录可以留在本机，不参与正式数据。
