# src-tauri/ Rust 后端约定

> 适用范围：`src-tauri/`（Tauri 主进程，managed_launch / codex_control / managed_gateway / model_exposure / database / conversation_archive 及 `codex-launcher`、`codex-gateway` sidecar bins）。跨端合同（路由、Gateway 协议、写入边界、安全隐私、发布流程）见仓库根 [AGENTS.md](../AGENTS.md)；前端侧见 `src/AGENTS.md`。

## 验证深度按风险选择

- 领域或纯函数变更：针对性 Rust 单测和格式/编译检查。
- Gateway、协议、异步流、诊断、迁移或生命周期变更：覆盖成功、上游失败、本地失败、取消、超时、恢复和隐私边界。
- 启动、默认 App、凭据文件或真实 Provider：记录平台、Codex 版本、配置来源和手动验收结果；不能用 mock 结果代替真实合同验收。

命令在仓库根执行（定义见根 `package.json`）：`pnpm run check:rust`（fmt --check + cargo check --all-targets）、`pnpm run test:rust`（--lib --bins -- --skip managed_gateway，与 CI test.yml 一致）。

## 构建与 sidecar

- Tauri 构建脚本会先构建 `codex-launcher` 和 `codex-gateway`，再嵌入 `dist`。遇到原生包仍显示旧页面时，按 README 说明清理 Cargo 构建缓存后重试。
- 生成的 macOS 包当前可能是 ad-hoc 签名，不能当作 Developer ID 公证发布包。AI 不运行打包命令，除非 commander 明确要求（见根 AGENTS.md）。

## 数据与迁移

- 迁移只更新 R Codex 自有库，additive 优先；schema 只增不降，回退约束见 `docs/CURRENT-STATE.md`（旧二进制拒绝更高 schema）。
- 单一写入路径：所有 SQLite 写经对应 Store 单事务；UI/renderer 不直接写库或凭据文件。
