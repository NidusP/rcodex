# 数据库凭据存储与 Keychain 移除

> 状态：已实施；Rust 与前端自动化验收通过，待真实 App 手动重新录入验收
>
> 日期：2026-10-05

## 目标

Provider API Key 改由 R Codex 自有 SQLite 存储，实例 Gateway 准入密钥彻底脱离 Keychain，运行时不再访问 macOS Keychain，避免授权弹窗和 `-25293` 启动失败。

用户接受重新手动录入所有 Provider API Key；本次不读取、不迁移、不恢复旧 Keychain 内容。

## 范围

- 新增专用 Provider 凭据表，按不可变 Provider revision 保存 API Key。
- ProviderStore、Gateway sidecar、launcher 统一改为数据库/一次性启动交接路径。
- 实例准入密钥只在 SQLite 保存哈希；明文只存在于当前启动进程、一次性 FIFO 和 Codex 子进程环境。
- 删除运行时 Keychain、`keyring` 依赖、Keychain fallback、Keychain 参数和旧凭据清理路径。
- 只清理明确属于 R Codex 的旧 Keychain 条目，不触碰默认 Codex 或其他应用凭据。

## 非目标

- 不迁移旧 Keychain API Key。
- 不把实例准入密钥明文写入 SQLite 或普通文件。
- 不迁移官方 OAuth/ChatGPT 凭据。
- 不提供旧版本无损回滚；旧版本若仍依赖 Keychain，需要重新录入凭据。

## 存储合同

```sql
provider_credentials (
  secret_ref TEXT PRIMARY KEY,
  api_key TEXT NOT NULL,
  updated_at INTEGER NOT NULL
)
```

Provider metadata and revision rows remain the routing source. `ProviderStore` is the only credential writer. Reads are exact by `secret_ref`; Gateway and sidecar do not infer credentials by provider name or model name.

`instance_gateway_bindings.instance_key_ref` and `instance_key_hash` remain the admission contract. The clear path only revokes/deletes the database binding; it never calls Keychain.

## 一致性与失败行为

- Provider revision、credential row、configured/state updates commit in one SQLite transaction.
- Missing credential marks the Provider as `credential_missing`; it does not fall back to Keychain or a previous secret.
- Instance launch generates a new high-entropy key, persists only its hash, and hands the plaintext through a one-shot FIFO. Missing FIFO or hash mismatch fails closed.
- Database permissions are owner-only on Unix (`0600`). Windows ACL tightening remains a release-environment verification item. API Key values remain backend-only and are zeroized after use.

## 一次性旧凭据清理

Cleanup is an explicit, non-blocking operation before/alongside the first database-only build. It targets only known R Codex service/account pairs and known Provider refs. It must not use a wildcard delete or touch the default Codex credential namespace. Cleanup failure is reported but cannot block database-only startup.

## 执行切片

- [x] Schema v22 and database credential access.
- [x] ProviderStore atomic database writes and reads.
- [x] Instance admission key in-memory/FIFO path without a vault.
- [x] Gateway, sidecar, launcher and app argument cleanup.
- [x] Remove Keychain implementation and dependency.
- [ ] UI states for missing/re-entered credentials.
- [x] Rust tests, static leak checks, Unix database permission path and frontend build verification.

## 验收

- App, Gateway, sidecar and launcher perform no Keychain reads or writes.
- Existing Provider metadata without a database credential is shown as missing; entering a new key makes it usable after restart.
- Instance launch, admission, stream forwarding, stop, revoke and restart recovery continue to work.
- No plaintext instance key appears in SQLite, logs, IPC snapshots, diagnostics or request archives.
- `cargo fmt --all -- --check`, `cargo check --all-targets`, focused Rust tests, `pnpm run check` and `pnpm run build` pass.

## 当前状态

- Rust 全套自动化通过：297 个库测试、1 个 Gateway 测试、13 个 launcher 测试、16 个官方认证集成测试均通过。
- `pnpm run check` 与 `pnpm run build` 通过；构建仅报告既有大 chunk 提示。
- 运行源码静态扫描未发现 Keychain API、`keyring` 依赖、Keychain fallback 或实例密钥 vault；R Codex 专属旧条目 `service=io.rcodex`、`account=rcodex-secrets` 已定向删除。
- 未启动 Tauri 开发窗口、未打包或发送真实 Provider 请求。commander 仍需重新录入 API Key，并在目标 macOS/Windows 环境手动验证启动、转发、停止和重启恢复。
- 上一个 Gateway “空闲中/转发中”改造的共享工作区变更需保留，并在最终构建中一并审查。
