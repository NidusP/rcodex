# 条件判断统一管理

状态：已按清单落地。数据库和 IPC 字符串未改。UI 第 6 步仍暂停。

验证：pnpm run check 通过。cargo test --lib 的 launch_target_tests、product_kind_tests 通过。managed_launch 相关 29 个测试通过。cargo check --bins --tests 与 cargo fmt --all -- --check 通过。

## 原则

数据库、IPC 和启动参数里的字符串保持原样，例如 `managed_app`、`cli`、`direct_responses`。比较和分支收到枚举或谓词后面。`#[cfg(target_os = "macos")]` 继续留在编译期，不做成运行时的 `is_macos()`，否则未编译进包的分支会丢失。

已有、本轮不重造：

- Rust `TransportMode`：`src-tauri/src/providers.rs`。
- Rust `managed_app_compatibility_enabled`：`src-tauri/src/codex_control.rs`。
- 前端 `isActiveInstanceState`、`CredentialKind`、`InstanceState`、`TransportMode`：`src/codex-control.ts`。
- 前端 `usageObservationLabel`：`src/workspace-surface.tsx`。

## 新增

| 名称 | 作用 | 维护文件 | 替换掉的散落判断 |
| --- | --- | --- | --- |
| `LaunchTarget` | 启动目标。`managed_app` 与 `cli` 的解析、写回和 `is_managed_app()`。 | `src-tauri/src/launcher_protocol.rs` | `managed_launch.rs`、`lib.rs`、`preset_validation.rs`、`codex_control.rs`、`bin/codex-launcher.rs` 里的 `launch_target == "managed_app"` |
| `EntryKind` | 启动方案种类。和启动目标共用 `managed_app` 这个词，但是另一件事：这是不是 3R 工作区方案。 | `src-tauri/src/codex_control/models.rs` | `managed_launch.rs` 里反复写的 `entry_kind == "managed_app"` |
| `CredentialKind` | 凭据种类：`api_key`、`official`、`none`、`managed_route_set`。提供 `is_api_key()`、`is_official()`。 | `src-tauri/src/codex_control/models.rs` | `codex_control.rs`、`managed_launch.rs`、`lib.rs`、`codex-launcher.rs` 的凭据字符串比较 |
| `InstanceState::is_active` / `is_finished` | 实例是否仍占着运行位，以及是否已退出或失败。 | `src-tauri/src/codex_control/models.rs` | `managed_launch.rs` 里 `exited`、`failed`、`running` 的组合判断 |
| `isManagedAppTarget` / `launchTargetLabel` | 前端判断是不是隔离 App，并给出「隔离 Codex App / 隔离 Codex CLI」。 | `src/codex-control.ts` | `App.tsx` 里同一套三元表达式 |
| `findManagedWorkspace` | 从方案列表取出 `entryKind === "managed_app"` 的 3R 工作区。 | `src/codex-control.ts` | `App.tsx` 里三处相同的 `find` |
| `isOfficialCredential` / `credentialKindLabel` | 官方、API Key、未配置的判断和短标签。 | `src/codex-control.ts` | `App.tsx` 与 `codex-control.ts` 里的凭据分支 |
| `isDirectTransport` / `isGatewayTransport` | 直连不观测用量；Gateway 才有超时和透传。 | `src/codex-control.ts` | `App.tsx`、`workspace-surface.tsx` 里的 `direct_responses` 比较 |
| `usesOfficialModel` | 方案或模型来源是官方，当前不能启动。 | `src/codex-control.ts` | `App.tsx` 启动前的官方来源检查 |

## 平台分支

`target_os = "macos"` 没有运行时字符串比较，全是编译开关。不新增 `is_macos()`。已经有名字的继续用：

- `managed_app_compatibility_enabled`、`managed_app_runtime_probe`：`src-tauri/src/codex_control.rs`
- `codex_app_bundle_candidates`、`bundled_codex_cli_candidates`：`src-tauri/src/codex_installation.rs`
- `process_start_identity`：`src-tauri/src/launcher_protocol.rs`
- `open_official_authorization_url`、`launch_codex_in_terminal`：`src-tauri/src/lib.rs`

`lib.rs` 里其余 macOS 专用块先保持 `cfg`，只有发现同一段逻辑复制了两次，才抽成上面这些文件里的具名函数。

## 顺序

先加类型和谓词，再按文件替换调用。存储值和 IPC 字段名不变。每换一个文件跑现有相关测试，不顺手改 UI。
