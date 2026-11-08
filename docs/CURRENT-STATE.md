# R Codex 当前状态

> 状态：active（最后核对 2026-11-08）
> 状态说明：本页是当前状态的唯一入口。AI 开工前先读本页；每次任务交接更新「阻塞与未完成」和「最近交接」两节，其余各节只在合同或架构变化时修改。行为约定见 [AGENTS.md](../AGENTS.md)。

## 1. 当前产品合同要点

以下只列最有约束力的条目；完整合同以各文档为准：

- 只保留 R Codex 工作区；受管理 App/CLI 一律独立 `CODEX_HOME`、独立模型目录与凭据，不读用户原 config.toml / auth.json / Cookie（见 [独立工作区改造](designs/2026-09-21-isolated-only-product-plan.md)）。
- 所有受管理请求走本机回环 Gateway（`codex-gateway` sidecar，仅 127.0.0.1）；第三方请求删除官方认证，凭据只从自有 SQLite `provider_credentials` 读取。
- 渠道发现完整目录，用户勾选「透出」后模型才可进入隔离空间；新启动的图片能力只取当前 supported 证据（见 [模型透出与图片探针](designs/2026-11-07-model-exposure-image-probe.md)）。
- 隔离空间固定 A–E 槽位，最多 5 个；新增创建空空间（`needs_model_selection`，选好前不可启动），不复制现有空间；彻底删除须确认且该空间全部实例停止（见 [排序与删除](designs/2026-11-07-workspace-order-and-deletion.md)、[多实例](designs/2026-10-04-multiple-managed-instances.md)）。
- 官方订阅模型路由保持关闭；当前版本 0.1.7（发版流程见 AGENTS「开发、测试与打包」）。

## 2. 架构地图

| 模块 | 职责 | 入口 |
| --- | --- | --- |
| `src/App.tsx` | 根组装：标题栏、隔离空间、抽屉/弹窗挂载 | `src/components/` 按功能目录 + `index.ts` 出口 |
| `src/use-app-controller.ts` | 前端状态控制器（快照轮询、写入命令、pending） | 各组件经 `useAppState()` 读写 |
| `src/workspace-surface.tsx` | 工作区卡片、空间 Tab、控制带、校准选择器 | `.workspace-*` 样式在 `src/workspace-surface.css` |
| `src/codex-control.ts` | 快照类型归一化、实例状态工具 | |
| `src/components/model-utility/` | 模型渠道、实例、诊断抽屉与确认弹窗 | `ControlDrawer`/`ControlDialog` |
| `src-tauri/src/managed_launch/` | 启动方案/空间：创建、绑定、启动准备、删除服务 | `workspace.rs`、`deletion.rs` |
| `src-tauri/src/codex_control/` | 实例生命周期、快照、CLI/App 启动 | `lifecycle.rs`、`snapshot.rs` |
| `src-tauri/src/managed_gateway/` | 回环 Gateway：路由、协议适配、压缩 | 数据路径 `Codex → 127.0.0.1 → Provider` |
| `src-tauri/src/model_exposure/`、`model_image_probe.rs` | 透出偏好与按模型图片探针 | schema v24 |
| `src-tauri/src/conversation_archive/` | 完整请求档案四阶段采集与按 scope 删除 | |
| `src-tauri/src/database/` | SQLite 迁移（当前 v24）与自有库 | 单一写入路径 |

UI 覆盖 Mantine 样式的注意点：类名前缀是 `rcodex-`（非 `mantine-`），变体覆盖需更高特异性；新增规则用无头探针实测（见 [UI 变更留痕](ui-change-log.md) 2026-11-07 条目）。

## 3. 写入路径

- SQLite：所有写经对应 Store（`ManagedLaunchStore`、`ModelExposureStore`、`providers/store.rs` 等）单事务；UI 不直接写库或凭据。
- 凭据：渠道 Key 事实来源是自有 SQLite `provider_credentials`；API 登录 Key 仅额外投影自有 `auth.json`（0600、原子替换）。实例准入密钥明文只经内存与一次性 FIFO。
- 空间配置：工作区修改保存后提示「重启可应用修改」；运行中实例继续用启动时固化的不可变 RouteSet 快照。

## 4. 验证命令

```bash
pnpm run check:all   # check:ui（tsc）+ check:rust（fmt/check）+ check:docs（文档链接）
pnpm run build       # 前端生产构建
pnpm run test:rust   # cargo test --lib --bins -- --skip managed_gateway（与 CI 一致）
pnpm run ui-check    # 无头浏览器夹具：workspace-ui-check + model-exposure-ui-check
```

验证深度按风险选择，规则见 [AGENTS.md「开发、测试与打包」](../AGENTS.md)。

## 5. 当前阻塞与未完成

- **CI Test 工作流长期失败**（自 2026-10-16 起每次 run 均失败，macOS exit 101 / Windows exit 1）；本地同命令全过，具体用例名需日志权限。0.1.7 发版不受影响，但待修复。
- **Windows 0.1.7 实机验收未做**：安装/更新后旧实例目录保留、双隔离 App 不同 Electron 数据目录、卸载不删 `%APPDATA%\io.rcodex`、空间删除（含文件锁重试）。安装器见 release v0.1.7。
- **0.1.6 → 0.1.7 回退需升级前数据库备份**：schema v24 旧二进制拒绝。
- 真实 Codex App 的独立 API 登录、多渠道连续会话、重启与导入后恢复验收；不得读取默认认证验证。
- 隔离 Browser/Computer Use：Windows 0.1.5 隔离 App 上仍禁用，不要再归因于目录开关、混用斜杠或 CLI symlink；Chat 自动审核仍不指向自己。
- Windows 打包、签名/公证、凭据文件权限，以及散装 DirectExe 的虚拟机验收。
- 官方账户与订阅模型路由仍关闭；不接管原 Codex 登录态。
- 跨渠道旧历史的字段兼容性和旁路模型须按真实合同验证；不能通过改源历史、自动换渠道或回退默认上游修复。
- Gateway 重启时的流中途续接；项目导入外部附件搬运、自定义源 SQLite 位置和异常中止遗留副本清理不属于首版。
- 官网 Pages 未开启，website workflow 在守卫步快速失败（预期行为）。

## 6. 最近任务交接

- **2026-11-08 子项目 AGENTS.md**（commander 批准）：新增 `src/AGENTS.md`、`src-tauri/AGENTS.md`、`website/AGENTS.md`（agents.md 嵌套规范，就近生效）；根 AGENTS.md 149 行只留跨端合同，子项目约定见各文件。
- **2026-11-09 修复 CI 测试**（CI run #16 日志定位，Test 工作流自首次 run 起从未通过）：① macOS supervisor 在子进程瞬间退出时读取进程身份失败导致启动报错，改为跳过 running 回执直接落 exited 回执；② Windows archive 在 append/只读句柄上 sync_all/set_len 被 Access denied（FlushFileBuffers 需要 FILE_WRITE_DATA），12 项测试与真实 Windows 档案写入全挂，改为独立可写句柄；③ Windows Command 环境键不区分大小写，NO_PROXY 双拼写折叠为一份，断言改为平台感知。本地 CI 同款命令 371 项全过（supervisor 压测 5/5）；Windows 路径由 CI 验证。
- **2026-11-08 归档迁移**（commander 批准）：38 份 historical/archived 文档与 stitch 设计资产 `git mv` 至 `docs/archive/`（镜像 designs 子层，git 历史保留）；读取面收敛为约 20 份 active 文档；链接修复 134 处，`check-doc-links` active 0 失效。回滚：git mv 逆操作。
- 更早（2026-11-08）：发版 0.1.7（产物见 GitHub Release v0.1.7；多隔离空间/空间删除/模型透出，schema v24）与治理第一轮（本状态页、UI 留痕页、AGENTS 瘦身、聚合验证命令）与文档维护规则（AGENTS 规则节 + check-doc-links 缺标记/状态页/strict 门禁）——详细记录见 [治理提案实施记录](project-governance-proposal.md) 与 git 历史。
