# macOS 钥匙串授权与存储（历史方案）

> 本文保留为历史排障记录。2026-10-05 起，R Codex 不再运行时读取或写入 macOS Keychain；当前合同见 [数据库凭据存储与 Keychain 移除](2026-10-05-database-credentials-no-keychain.md)。

> 2026-10-02：当前合同是 macOS 上**模型渠道 API Key** 写入同一钥匙串条目 `3r-secrets`，迁完后一次授权即可长期使用。实例准入密钥不是用户凭据，不进钥匙串：启动时生成，哈希写入 SQLite，明文经一次性命名管道交给 CLI；Gateway 准入只比对哈希。

## 目标

- 用户提供的渠道 API Key 仍以系统凭据库为真源，不写入 SQLite、日志、IPC 快照或普通文件。
- 多渠道不再变成多条钥匙串、多次授权。迁完后主进程、sidecar、launcher 和同路径重建都只碰 `3r-secrets` 这一条。
- 实例准入密钥不申请钥匙串权限。

## 非目标

- 不改 Windows Credential Manager。
- 不在每次刷新时读取凭据；`configured` 列和进程缓存仍然有效。
- 不把渠道 API Key 降到明文文件来换取免密。

## 为何不能靠「始终允许」

登录钥匙串默认 ACL 绑的是调用方代码签名。开发态 ad-hoc 签名每次编译都会变。`tauri.conf.json` 不能固定签名，「始终允许」也不能跨重建复用。系统按**条目**授权，没有「一次批准这个 App 的全部密码」。

## 方案

### 模型渠道 API Key

一条通用密码：

- 服务名：应用 identifier（当前 `io.3r.codex`）
- 账户名：`3r-secrets`
- 内容：版本化 JSON 密钥包，按 `secret_ref` 存放渠道 API Key
- ACL：`SecAccessCreate` 且 trusted list 为空，本机应用可读，不绑代码签名

读取顺序：进程缓存 → `3r-secrets` → 旧的 `3r-provider-secrets` 包 → 旧的按渠道分散条目。主进程命中旧条目后写入 `3r-secrets` 并删除分散条目。sidecar 只读密钥包，不改 ACL。启动 Gateway 前，主进程先把已有渠道密钥迁进包内。

把尚未合并的旧渠道条目搬进去时，系统仍可能对每条旧条目各要一次授权；迁完之后不再按渠道数弹窗。

### 实例准入密钥

实例密钥是本次 Gateway 绑定的随机准入令牌，不是用户保存的 API Key。

- 启动时在主进程生成，明文只存在内存和一次性 FIFO 里，交给 `codex-launcher` 再注入 CLI 环境。
- SQLite 只保存哈希，Gateway 用哈希核对请求头。
- 不写入钥匙串。sidecar 恢复绑定也只使用库里的哈希，不再回读明文。
- `codex-launcher` 只从 FIFO 读取实例密钥；管道缺失时直接失败，不再回退钥匙串。
- 停止实例时若钥匙串里还有旧版残留条目，则尽力删除。

## 验收

- 渠道密钥包迁完后，启动多渠道实例只申请（或不申请）`3r-secrets`，不再按渠道数连弹。
- 新建或启动实例不因实例密钥申请钥匙串权限。
- 新保存的渠道密钥写入密钥包，不新建分散条目。
- 不必手动重新保存模型渠道。
- SQLite、诊断、IPC 快照仍不含密钥原文。
- `cargo test --offline --lib secrets` 与 `cargo check --offline --all-targets` 通过。
