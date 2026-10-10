# R Codex

R Codex 是本地优先的 Codex 模型渠道与启动管理工具。配置多个 API 模型渠道、选择可用模型和一个 API 登录渠道，然后点击启动。R Codex 启动的 Codex 使用完全独立的配置、登录、会话索引和桌面用户数据。

用户自己打开 Codex 时仍使用自己的环境。R Codex 不读取或修改原 config.toml、auth.json、Cookie 或桌面登录数据；仅在用户主动导入项目会话时只读访问源会话。不再提供默认 App / 隔离 App 双入口，也不提供默认配置清理向导。旧版本已经写入的兼容配置不会因升级自动还原。

当前产品合同与验收：[独立工作区改造计划](docs/designs/2026-09-21-isolated-only-product-plan.md)。技术标识为 rcodex；3r-gpt 只用于历史迁移兼容。

## 使用流程

1. 添加 API Key 模型渠道，填写服务地址与密钥，检测并选择模型。
2. 在 R Codex 工作区加入多个渠道模型，设置默认模型，选择一个渠道作为 API 登录。单渠道可自动选中；多渠道需明确选择。
3. 点击主启动图标，悬停显示「启动」。CLI 可从旁边的终端图标启动。
4. 运行中配置锁定；退出 Codex 后再调整，下次启动使用新的路由快照。App 重启复用同一份 R Codex 会话空间。

R Codex App 通过本机 Gateway 按模型 ID 精确路由到对应渠道，登录渠道不会取代其他渠道的请求密钥。官方账户与订阅模型路由暂未开放。

## 导入我的 Codex 会话

只支持「我的 Codex → R Codex」按项目单向复制。先从 R Codex 启动 Codex 创建一次会话，退出后点击「扫描我的 Codex 项目」，选择项目并预览；确认所有 Codex App/CLI 已退出，再执行导入。

源配置、认证与会话保持原样。相同 ID 跳过，不覆盖；目标副本统一使用 rcodex Provider。源和目标索引版本须一致。附件引用仍指向本机原路径，不保证不同模型渠道可以无损续聊。首版不自动发现自定义 CODEX_SQLITE_HOME。

## 本地网关如何工作

~~~text
R Codex 启动的 Codex（独立环境）
    → 127.0.0.1:43123/v1
    → 按模型匹配启动时固定的渠道、地址与密钥
    → 对应模型服务
~~~

网关只监听回环地址，不提供局域网或公网入口。每个运行实例使用独立准入密钥。模型 POST 不自动重试，不自动更换渠道、模型或认证。未知模型与不支持的能力明确失败，不回退到用户原来的 Codex 渠道。

网关由独立 codex-gateway 伴随进程承载。有活动绑定时退出 R Codex 窗口不会直接终止转发。重启会核验和恢复独立运行实例的绑定，不恢复旧默认 App 兼容绑定；正在传输的流不保证跨网关重启续接。

控制面丢失 ready 文件时，关 CLI 不会自动停掉仍占用 127.0.0.1:43123 的 sidecar；恢复步骤见 [孤儿 Gateway](docs/designs/2026-10-09-orphan-gateway-sidecar.md)。

R Codex 拉起的进程补充回环 NO_PROXY/no_proxy，系统代理和用户默认环境保持不变。

## 转发策略

第三方模型要能在 Codex 里用，Gateway 只做渠道真正支持的适配，不冒充没有的能力。

- 按公共模型 ID 精确匹配启动时固化的路由，再换成上游模型名。不按模糊名字猜渠道，也不因 401 或 502 自动换地址、换模型、换渠道或重试这次 POST。
- 第三方请求去掉官方认证，只带这一条渠道自己的 API Key。
- 为了中继能收下请求，去掉 Responses 的 metadata。渠道不支持的可选工具（例如网页搜索）从这次请求拿掉，而不是假装支持。
- Codex 私有的 agent_message 收成公开的 user 消息。读不懂的私有项、不支持的图片，以及隔离实例上的远程压缩触发，明确失败，不回退到用户原来的 Codex。
- 隔离实例的长对话压缩由 Codex 本地完成。Gateway 只转发普通请求，并展开自己的压缩信封；不重排用户消息，也不判断哪一句是当前任务。
- 只有路由宣布了 Responses→Chat 时才做文本转换，不伪造工具、图片或多轮能力。

## 数据与安全边界

- R Codex 元数据保存在应用自己的 SQLite，当前 schema v18；原默认 Codex 的认证、会话和 MCP 不参与启动快照，安全白名单偏好可在首次建立或显式同步时复制。
- 渠道密钥保存在系统凭据库。为初始化 API 登录，选定 Key 仅投影到 R Codex 自有 auth.json，私有权限、原子更新；不接管原登录态。
- 独立配置显式使用文件凭据存储，不共用默认认证。默认历史仅在显式项目导入时只读复制。
- 普通诊断记录状态、耗时、Token 与脱敏错误，不记录正文。完整请求档案在「运行诊断」中单独开启、按阶段分页读取和清空；正文只存本机独立档案。
- 升级只在 R Codex 数据库中停用旧兼容绑定与资源锁，不还原默认配置、不删除用户文件、不关闭原 Codex。

## 开发与打包

~~~bash
pnpm run check
pnpm run build
pnpm run website:dev
pnpm run tauri:dev
pnpm run tauri:build --bundles app
~~~

~~~bash
cd src-tauri
cargo fmt --all -- --check
cargo check --all-targets
cargo test --all-targets
~~~

Tauri 脚本先构建 codex-launcher 和 codex-gateway，再嵌入前端。这两个附带程序按目标三元组放在 `src-tauri/binaries/`，不提交。`build.rs` 在编译附带程序时就会检查对应文件存在，所以干净环境里脚本会先放占位文件，编译完成后再换成真正的二进制。本机如果已经有上次构建留下的文件，不会先遇到这个错误。若原生包显示旧页面，按需执行 cargo clean --manifest-path src-tauri/Cargo.toml 后重新打包。生成的 macOS 包可能为 ad-hoc 签名，不能视为已公证发布包。

## 应用更新

R Codex 用 GitHub Release + Tauri updater 检查自己的新版本，不依赖 Apple 公证。发版仓库写在 `src/app-release.ts` 的 `APP_GITHUB_REPO`，并与 `src-tauri/tauri.conf.json` 的 updater endpoints 保持一致。

私钥在 `work/updater.key`，不要提交。GitHub Secrets 需要 `TAURI_SIGNING_PRIVATE_KEY` 和 `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`（当前为 `rcodex-updater`）。打 tag `v0.1.x` 后 `.github/workflows/release.yml` 会构建 macOS 包并创建 draft Release。

启动后若有新版本，通知栏和标题栏可以「安装并重启」。有隔离实例在跑时会拒绝安装。

应用图标源文件为 `src-tauri/icons/icon.png`，由 Tauri CLI 生成 macOS 的 `icon.icns`、Windows 的 `icon.ico` 和各尺寸 PNG：

~~~bash
pnpm exec tauri icon src-tauri/icons/icon.png -o src-tauri/icons
~~~

`src-tauri/tauri.conf.json` 的 `bundle.icon` 显式引用这些平台资源。源图四角透明、主体为白色圆角底并保留黑色艺术字；源图内容保留约 8% 的透明安全边距，使 macOS 图标显示尺寸与其他应用一致。重新生成图标后需保留同样的透明圆角和安全边距。

目标平台为 macOS 与 Windows；当前打包脚本只接受 macOS，Windows 发布链路与系统凭据权限仍需验收。浏览器夹具和自动化测试不能替代真实 Codex App 登录、模型切换及会话恢复验证。

详见[产品原理](docs/3rcodex-product-principles.md)、[架构设计](docs/codex-hybrid-routing-design.md)、[执行与验证记录](docs/codex-hybrid-routing-execution-plan.md)。
