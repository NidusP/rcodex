# Windows 安装根、DirectExe 与隔离 CLI

> 状态：2026-10-17 源码已落地。散装 DirectExe 与 Windows 隔离 CLI 终端仍需虚拟机验收。商店包隔离启动已接通。本方案把「商店 / exe」收成同一份 Codex 安装，只在拉起进程时分叉；Windows 隔离 CLI 用 Windows Terminal 或 cmd start 打开，不再返回仅 macOS。发现层桌面优先于 PATH、隔离首启打开 Browser / Computer Use 已在同日落地。

## 目标

Windows 上只有一份 Codex Desktop 合同：独立 CODEX_HOME、独立 Electron 用户数据目录、`--user-data-dir=`、本机 Gateway、实例准入密钥。用户从商店装、从官网 exe 装、或装到自选目录，隔离 App 的配置和工具相同。

分叉只发生在操作系统如何创建进程：

- 路径在 `WindowsApps` 且有包家族：`Invoke-CommandInDesktopPackage`。
- 否则：对桌面 `ChatGPT.exe` 做 `CreateProcess`。

不要按「是不是商店下载」写第二套产品逻辑。官网 exe 若实际是同一份 MSIX，安装器会提示已安装，运行时仍走包身份。

隔离 CLI 与隔离 App 共用同一安装解析和独立 CODEX_HOME；差别只是可见终端托管 launcher 的 --target cli。

## 非目标

- 不把商店包 `CreateProcess` 化，也不把散装 exe 走 `Invoke-CommandInDesktopPackage`。
- 不复制安装树，不要求管理员权限。
- 不把 macOS `.app` 合同搬到 Windows。
- 不在本方案里改 Gateway、模型目录、Node REPL overlay。
- 没有散装安装时，不把 DirectExe 标成已验收。

## 安装根

发现层解析出一个安装，而不是分别解析「商店 CLI」和「普通 CLI」：

```text
WindowsCodexInstall {
  spawn: Package | Direct
  root:  安装根目录
  cli:   命令行运行时
  gui:   桌面窗口
}
```

`root` 由 CLI 或 GUI 推出，允许用户自选盘符和文件夹。禁止写死 `LocalAppData\Programs`。

相对 `root` 的布局按先命中先用，不按来源渠道分表：

| 角色 | 候选 |
| --- | --- |
| GUI | `app\ChatGPT.exe`，然后 `ChatGPT.exe`，然后 `app\Codex.exe`（仅当它不是 CLI） |
| CLI | `app\resources\codex.exe`，`resources\codex.exe`，`app\resources\codex-cli\codex.exe`，根目录 `codex.exe` |
| Computer Use 运行时 | `app\resources\cua_node\...` 或 `resources\cua_node\...` |

规则：

1. GUI 不得与 CLI 大小写不敏感相同。`resources\codex.exe` 永远不是窗口。
2. CLI 落在 `...\resources\codex.exe` 时，窗口只从上一层 `app\` 生成，不生成 `resources\ChatGPT.exe`。
3. `spawn`：`root` 或 `gui` 含 `\WindowsApps\`，或 `Get-AppxPackage` 能对应到这个 `root`，则为 `Package`；否则 `Direct`。未注册的商店路径失败，不回退 `CreateProcess`。
4. 发现顺序：`CODEX_BIN`，已安装桌面（`%LOCALAPPDATA%\Programs` 常见名，然后商店 `InstallLocation`），PATH，用户目录 npm。PATH 上的 npm `codex` 不得盖住桌面安装。

## DirectExe 的 CreateProcess

仅 `spawn = Direct` 使用。可执行文件是 `gui`，不是 CLI。

| 项 | 合同 |
| --- | --- |
| 可执行文件 | `gui`（ChatGPT.exe） |
| 工作目录 | `gui` 所在目录（安装根或 `app\`）。不要用用户项目目录当 cwd，避免 Chromium/Electron 按 cwd 找 DLL |
| 参数 | 先去掉已有 `--user-data-dir` / `--user-data-dir=`，再追加 `--user-data-dir=隔离electron目录`。必须用等号形式 |
| 项目目录 | 若需要打开某个文件夹，放在独立参数里，不替代 cwd |
| 环境 | `CODEX_HOME`、`CODEX_SQLITE_HOME`、`CODEX_ELECTRON_USER_DATA_PATH`、实例密钥；删除 `OPENAI_API_KEY` / `CODEX_API_KEY` / `CODEX_ACCESS_TOKEN`；回环写入 `NO_PROXY` / `no_proxy` |
| 控制台 | GUI 不设 `CREATE_NO_WINDOW`。探针和 powershell 仍用无窗口 |
| 禁止 | `ShellExecute`、开始菜单 AppID、`chatgpt:` 协议、把隔离环境传给「打开默认 Codex」 |

监督、回执、停止实例与现在 launcher 相同。R Codex 退出仍先停隔离实例。

## 打开默认 Codex

与隔离启动共用 `WindowsCodexInstall`，但：

- 不设 `CODEX_HOME` / `CODEX_SQLITE_HOME` / `CODEX_ELECTRON_USER_DATA_PATH` / 实例密钥；
- 不传隔离 `--user-data-dir=`；
- Package 用 explorer.exe shell:AppsFolder\{family}!App，等同开始菜单打开默认窗口；Direct 用 CreateProcess(gui) 且 cwd 仍是 gui 所在目录。

删除「商店路径请用非商店安装」这种按下载渠道拒绝的错误。商店用户打开默认窗口应成功。隔离启动仍走 Invoke-CommandInDesktopPackage，不要把默认入口和隔离入口混成一条。

## 隔离 CLI

控制面不再对非 managed_app 返回「仅 macOS」。隔离 CLI 仍走 codex-launcher 的 --target cli 与现有 argv、一次性密钥文件、回执；可见终端只负责托管这个 launcher。

| 项 | 合同 |
| --- | --- |
| 有 Windows Terminal | wt.exe -d <项目目录> -- <codex-launcher> <argv> |
| 没有 wt | cmd.exe /c start "R Codex CLI" /D <项目目录> <codex-launcher> <argv> |
| 项目目录 | 用户选的工作目录。没有则省略 -d / /D。不要用 GUI 目录当终端 cwd |
| 控制台 | 这条命令不能走 command_without_console / CREATE_NO_WINDOW。探针和商店激活脚本仍用无窗口 |
| launcher | 正式包是 Windows GUI 子系统。CLI 目标先 AttachConsole(ATTACH_PARENT_PROCESS)，失败再 AllocConsole()，然后把 stdio 交给 codex.exe |

App 启动仍直接 spawn codex-launcher.exe（可无窗口）。商店包激活仍在 launcher 里，不在控制面。

## 代码改动

现有分叉散落在发现、launcher、「打开默认 Codex」和 Windows CLI 拒绝。收成安装根之后，launcher 只问 spawn；CLI 只问终端宿主。

1. `src-tauri/src/codex_installation/windows.rs`  
   增加 WindowsCodexInstall 与 install_from_cli()。把 program_cli_candidates、store_cli_candidates 收成对 root 的相对布局。保留任意目录样本单测，例如 `D:\Apps\Codex\app\resources\codex.exe` → gui 为 `D:\Apps\Codex\app\ChatGPT.exe`，spawn 为 Direct。

2. `src-tauri/src/codex_installation/mod.rs`  
   `resolve_codex_executable` / `resolve_codex_app_executable` 改为安装解析的入口，不再让调用方自己猜商店。Windows 桌面优先于 PATH 的顺序保持。

3. `src-tauri/src/managed_app_launch/windows.rs`  
   DirectExe 增加 gui_directory 与启动计划：cwd、唯一 user-data-dir、项目目录只进 argv。打开默认 Codex 的 Package 走 AppsFolder。

4. `src-tauri/src/bin/codex-launcher.rs`  
   受管理 App：Package 走现有 activate_store_package；Direct 的 cwd 是 gui 目录，项目目录只 .arg()。CLI 目标补控制台。

5. `src-tauri/src/commands/system.rs`  
   `launch_codex_in_app` 去掉商店拒绝，改用同一安装解析的非隔离拉起。

6. `src-tauri/src/managed_codex_config/computer_use.rs`  
   Windows helper 从 CLI 推出的 app 目录解析 cua_node；没有 resources 段时回退 CLI 父目录，不要假设只有商店 `app\resources`。自选目录若是扁平布局，走 `resources\cua_node`。

7. `src-tauri/src/managed_spawn/windows.rs` 与可在 macOS 单测的 `windows_cli.rs`
   CLI 按 wt / cmd 合同组装 argv 并 spawn。App 保持现有 spawn launcher。

调用方（`managed_launch/prepare.rs`、探针、打开隔离目录）继续只拿 CLI/GUI 路径，不出现「商店用户 / exe 用户」分支。

## 实施顺序

1. 安装根类型与相对布局，单测锁住商店 `WindowsApps\...\app\resources\codex.exe` 和自选 `D:\Apps\Codex\...`。行为与现在发现结果一致。
2. DirectExe：cwd = gui 目录，`--user-data-dir=` 去重。launcher 接上。
3. 打开默认 Codex 共用安装解析。
4. Computer Use 按 app 目录找运行时，扁平布局可回退。
5. Windows 隔离 CLI：wt / cmd argv 单测，launcher CLI 目标补控制台。
6. Windows 实机：商店包隔离、默认入口、隔离 CLI 终端；若有散装自选目录则再测一条 DirectExe。没有散装包时，用夹具路径测布局，不把 DirectExe 标成已验收。

## 验收

- 商店：`status.json` 的 `codexExecutable` 以 `\app\ChatGPT.exe` 结尾，状态保持 `running`，隔离目录不是默认 `\.codex`。
- 自选目录夹具：从 `...\app\resources\codex.exe` 推出的 gui 不是 `resources\codex.exe`；spawn 为 Direct；root 为安装根。
- DirectExe 命令行含且仅含一份 `--user-data-dir=`，cwd 为 gui 父目录，项目目录只在 argv。
- 「打开默认 Codex」在商店安装上不再报请改用非商店包；Package 走 shell:AppsFolder 与包家族 AppID。
- 隔离 CLI：有 wt 时命令含 -d、--、launcher 和 --target cli；无 wt 时为 cmd.exe /c start "R Codex CLI"。
- `cargo fmt --check`、Windows 路径与 Computer Use 相关单测、`cargo test --lib --offline` 通过。

## 回滚

恢复 launcher 对 DirectExe 的当前 `Command::new(gui)` + 项目目录 cwd；恢复「打开默认 Codex」拒绝 WindowsApps；Windows managed_spawn 对 CLI 再返回仅 macOS。安装根类型可留作发现层内部结构，不接启动。

## 与已落地改动的关系

- 发现层桌面优先于 PATH：已在 `codex_installation/mod.rs`。
- 隔离首启打开 Browser / Computer Use / Ask for approval：已在 `preferences.rs`。
- 本方案已写入源码：安装根类型、DirectExe cwd、默认入口去商店拒绝、Windows 隔离 CLI 终端。散装 DirectExe 与 CLI 终端未在 Windows 实机验收。
