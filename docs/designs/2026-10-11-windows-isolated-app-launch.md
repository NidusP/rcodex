 # Windows 隔离 App 启动

> 状态：普通 Win32 路径已接通，可供安装包外发测试。商店包激活仍关闭。

## 目标

在 Windows 上按现有隔离合同启动受管理 Codex Desktop：独立 `CODEX_HOME`、独立 Electron 用户数据目录、`--user-data-dir=`、本机 Gateway、实例准入密钥。商店安装与普通 Win32 安装都能启动，且不会落到用户默认 Codex 窗口。

macOS 继续用现在的 `.app` + FIFO + `/bin/sh` 或直接 spawn launcher。两端共享准备目录、RouteSet、回执和监督，不共享 OS 启动方式。

## 非目标

- 不复制 Cockpit 的多账户池、OAuth relay 或 PowerShell 长期控制面。
- 不把 macOS 短 `CODEX_HOME` 迁到 Windows。
- 首版不实现 Windows 受管理 CLI 的 `wt.exe` / `cmd.exe` 终端包装；CLI 按钮可继续提示未支持。
- 不在没有 Windows 实机时宣称商店包激活已验收。
- 不把 `Invoke-CommandInDesktopPackage` 用到未注册的普通 exe 上。

## 合同

控制面只负责准备隔离目录和调用 `codex-launcher`。真正拉起 Codex GUI 的是 launcher。

| 步骤 | 所有者 | macOS | Windows |
| --- | --- | --- | --- |
| 准备 home / Electron 目录 / Profile | `managed_launch` | 短真实目录 | 长 `codex-home`，已有 |
| 发现 CLI 与桌面 GUI | `codex_installation` | `.app` / `Contents/MacOS` | `codex.exe`、LocalAppData、商店 `ChatGPT.exe` |
| 受管理 App 探针 | `codex_control/helpers` 改为调用发现层 | sidecar 无后缀 + `/Applications` | sidecar `.exe` + 已发现 GUI |
| 控制面拉起 launcher | 新建 `managed_spawn` | App：`/bin/sh` 临时脚本；CLI：iTerm/Terminal | App：直接 spawn `codex-launcher.exe` |
| 准入密钥 | `managed_spawn` | FIFO | Named Pipe 或仅当前用户可读的临时文件 |
| 进程身份 | `launcher_protocol` | `proc_pidinfo` | `GetProcessTimes` 或等价 |
| 拉起 GUI | `codex-launcher` | `Command` + 环境 + `--user-data-dir=` | 有包注册：包身份激活；否则 `CreateProcess` |

商店包路径必须走包身份。`WindowsApps\...\ChatGPT.exe` 未注册则失败，不回退默认实例。普通安装没有 `PackageFamilyName`，不得调用 `Invoke-CommandInDesktopPackage`。

## 代码组织

不要在 `commands/instances.rs` 里继续堆 `cfg(target_os)`。把「发现」「调度」「拉起 GUI」分成库模块，launcher 二进制变薄。

```
src-tauri/src/codex_installation.rs
        ↓ 扩成目录，现有 macOS 测试保持
src-tauri/src/codex_installation/
  mod.rs                 # resolve_codex_executable / resolve_codex_app_executable
  macos.rs               # .app 候选、Contents/MacOS、内嵌 CLI
  windows.rs             # PATH 的 codex.exe、常见安装目录、WindowsApps 文本路径
  windows_store.rs       # 包家族、manifest Application、路径切割；cfg(any(test, windows))

src-tauri/src/managed_spawn/
  mod.rs                 # launch_managed_launcher(...)，instances 只调这里
  macos.rs               # 现有 .command、FIFO、/bin/sh、open -a Terminal
  windows.rs             # 直接 spawn launcher.exe；密钥管道
  secret.rs              # 一次性密钥交接接口

src-tauri/src/managed_app_launch/
  mod.rs                 # configure_managed_app_command（现 launcher 里的环境/参数）
  windows.rs             # DirectExe vs PackageIdentity
  windows_package.rs     # 激活脚本、helper 回执；launcher.exe 兼任 helper
```

`commands/instances.rs` 只保留 IPC、锁、失败回滚。`commands/system.rs` 的「打开默认 Codex / 打开隔离目录」同样改走 `managed_spawn` 的平台文件，不在 command 里写 `open`。

`bin/codex-launcher.rs` 只解析参数、读密钥、调用 `managed_app_launch`、写回执、监督进程。不要在 bin 里写商店 PowerShell。

平台文件用 `#[cfg(target_os = "macos")]` / `#[cfg(target_os = "windows")]`。纯路径和参数组装用 `#[cfg(any(test, target_os = "windows"))]`，这样 Mac 上能跑 Windows 单测。禁止在共享函数里用 `Path::parent()` 解析 `\`。

## 分步

1. **只重构 macOS。** 把 `launch_codex_in_terminal`、FIFO、Terminal 选择挪到 `managed_spawn/macos.rs`，把 `configure_managed_app_command` 挪到 `managed_app_launch`。行为与测试不变。Windows 仍返回现有「仅 macOS」错误。
2. **发现层。** Windows 候选：`CODEX_BIN`、PATH 上的 `codex.exe`、`%LOCALAPPDATA%\Programs`、`WindowsApps\...\app\ChatGPT.exe`。从 GUI 反推 CLI、从 CLI 反推 GUI。商店路径用文本切割。Mac 单测覆盖路径样本。
3. **调度与密钥。** Windows 控制面直接 spawn `codex-launcher.exe`，不写 `.command`。密钥用 Named Pipe；协议与超时在 Mac 上测。`process_start_identity` 补 Windows。`HOME` 读取改为 `HOME` 或 `USERPROFILE`。
4. **GUI 拉起。** `managed_app_launch/windows.rs` 先查包注册：有则 `Invoke-CommandInDesktopPackage`，helper 为当前 `codex-launcher.exe`；无且不在 WindowsApps 则 `CreateProcess`。参数统一 `--user-data-dir=`，去掉重复项。用新 PID + 用户数据目录确认实例，避免认到默认窗口。
5. **探针与权限。** `managed_app_compatibility_enabled` 在 Windows 上检查 `.exe` sidecar 和已发现 GUI。`set_private_directory` 在 Windows 上设当前用户 ACL；没有实机前可先做空操作但测试要标明。打开隔离目录用 `explorer`。默认 Codex 用普通 exe 或包身份，且不带隔离环境变量。
6. **CI 与实机。** `windows-latest` 编译并跑新单测。商店激活、ACL、真实 `ChatGPT.exe` 只在 Windows 桌面记一次。在此之前 Windows 包继续显示未通过受管理 App 探针。

## 验证

- Mac：现有 launcher / instances 测试仍通过；新增 Windows 路径与参数测试在 Mac 上跑。
- CI Windows：`cargo test` 含 `any(test, windows)` 模块；release 能链上 `codex-launcher.exe`。
- Windows 桌面：商店 Codex 与普通安装各开一个隔离 App，目录不进默认实例；Dev 与生产身份仍分离。

## 回滚

恢复 `managed_app_compatibility_enabled` 非 macOS 为 false，Windows `managed_spawn` 回到显式错误。不删除已准备的隔离目录。


## 执行记录

- 2026-10-11：完成发现层目录拆分（`codex_installation/{mod,macos,windows,windows_store}.rs`），HOME 读取改为 HOME 或 USERPROFILE。Windows 控制面受管理 App 直接 spawn `codex-launcher.exe` 且 argv 不套 shell；CLI 仍返回原错误。商店路径无包注册时失败，不回退默认实例；helper 必须是 launcher。`managed_app_compatibility_enabled` 在 Windows 上仍为 false。商店激活、ACL、真实 ChatGPT.exe 只在 Windows 桌面记一次。

- 2026-10-16：对照当前源码，普通 Win32 的受管理 App 启动已经不靠硬编码 false 挡住。managed_app_compatibility_enabled 在 Windows 上走探针：当前进程旁要有 codex-launcher.exe 和 codex-gateway.exe，能发现 CLI 与同目录桌面 GUI，且 GUI 不在 WindowsApps。环境变量 THREER_CODEX_DISABLE_MANAGED_APP=1 仍是紧急关闭。launcher 对 DirectExe 用 CreateProcess；商店包 query_registered_launch 固定返回 None，即使路由到包身份也会拒绝启动。本次未改代码，也没有 Windows 桌面验收。

仍缺、不能当成正式放行的条件：

- 商店包激活没有真实 Get-AppxPackage 结果，不能启动 WindowsApps 里的 ChatGPT。
- 一次性密钥写成 launch-secret.txt，没有 Named Pipe，也没有只给当前用户的 ACL。set_private_directory 在 Windows 上仍是空操作。
- 受管理 CLI 仍返回仅 macOS；不在这次 App 放行里。
- 没有 Windows 桌面记录：普通安装打开隔离 App、目录不进默认窗口、Dev 与生产身份分离。
- 2026-10-16：虚拟机安装 0.1.2 后弹出终端，打印 managed gateway startup deferred: managed_gateway_sidecar_missing，关掉终端时应用一起退出；商店版 Codex 被报未找到 CLI。原因是正式包仍是控制台子系统，伴随进程只找不带 .exe 和目标三元组的文件名，CLI 发现也不查 Appx 安装位置。源码已改为正式 Windows 包不分配控制台，按 Tauri externalBin 文件名查找 codex-gateway 与 codex-launcher，并用 Get-AppxPackage 找商店包里的 codex.exe。0.1.2 安装包本身不会变，要下一版 Windows 包才能验证。商店包激活仍不启动隔离 App。
- 2026-10-16：虚拟机里的商店包 Name 是 OpenAI.Codex，不是 OpenAI.ChatGPT。PackageFullName 为 OpenAI.Codex_26.1013.3656.0_arm64__2p2nqsd0c76g0，InstallLocation 在 WindowsApps 下同名目录。C:\Users\xxx\.codex 只是配置目录。查找改为先 Get-AppxPackage -Name OpenAI.Codex，没有再回退 OpenAI.ChatGPT*。
- 2026-10-16：Windows 隔离 App 入口对商店包放开。开始菜单显示名是 ChatGPT，AppID 是 OpenAI.Codex_2p2nqsd0c76g0!App。探针不再因为 WindowsApps 拒绝；launcher 用 Invoke-CommandInDesktopPackage 拉起 codex-launcher，再由 helper 启动隔离桌面。本机是 macOS，没有 NSIS 和 Windows 链接器，打不出 exe 安装包。
