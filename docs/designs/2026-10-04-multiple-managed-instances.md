# 多隔离实例

> 状态：已实现。2026-10-04。类型检查与 Rust 单测通过。未启动 Tauri 开发窗口。
> 首页空间条的「新建空间」改为「新增实例」，并允许创建多个 managed_app 启动方案。

## 目标

用户可以在首页标签栏保留多个隔离实例。每个实例是一条 managed_app 启动方案：渠道和模型配置独立保存，首次启动后拥有自己的 CODEX_HOME、会话和 Electron 桌面数据目录。多个受管理 Codex App 可以同时运行。首页「新增实例」暂时隐藏，创建命令仍保留。

## 范围

- 首页标签列出全部未删除的 managed_app 启动方案。
- 「新增实例」可点。默认复制当前实例的渠道、模型和 API 登录渠道，名称按「实例 2」「实例 3」递增。
- 点标签切换当前编辑/启动对象；选中项写入 is_preferred，刷新后仍停留在该项。
- 编辑渠道和模型时带上当前实例 ID，不再把名称写回固定的「R Codex」。

## 非目标

- 同一条实例已经在运行时，不能再次启动这一条；该操作仍返回 managed_app_in_use。
- 首页暂时不显示「新增实例」。
- 不改变 Gateway 回环、凭据库和官方账户关闭合同。

## 合同

- Schema v21 删除 launch_presets_entry_kind 唯一索引，允许多条 entry_kind='managed_app' 记录。
- create_managed_launch_preset：
  - 默认仍更新当前选中（is_preferred）的 managed_app 方案，兼容旧客户端。
  - launchPresetId 更新指定方案。
  - createNew: true 插入新方案，并把它标为 preferred。
- select_managed_launch_preset 只切换 preferred，不增加 version，不改渠道和模型。
- 每个启动方案仍最多绑定一个受管理 App 运行时；CODEX_HOME 在该方案首次启动时分配。
- 写入仍走 ManagedLaunchStore::create_preset / select_preset，UI 不直接写库。

## 验收

- 现有「更新单个主工作区」测试继续通过。
- 新测试：createNew 插入第二条 managed_app 方案；v21 后唯一索引不存在。
- 前端类型检查通过。未启动 Tauri 开发窗口。
- 实机：新增实例后出现新标签，渠道从当前实例复制，启动仍受单 App 限制。

## 回滚

恢复 schema 版本 20、create_preset 的单方案 upsert，以及首页禁用「新建空间」。已创建的额外 managed_app 行不会自动删除。
