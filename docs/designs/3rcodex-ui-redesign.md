# 3R Codex UI Redesign

## Status

- Stage: first implementation slice complete; backend contracts unchanged.
- Reference: user-provided Cockpit Tools screenshot, used for interaction density and account-card patterns, not for branding or pixel copying.
- Primary surface: Tauri 2 desktop window with React/Vite frontend.

## Goal

Make the common workflow obvious and fast:

1. See whether the current Provider and Gateway are usable.
2. Add or switch an API-key Provider.
3. Test connectivity and understand the failure reason.
4. Start Gateway or launch Codex with one clear action.
5. Inspect usage and diagnostics without exposing secrets or burying the main action.

## Current Findings

- The current screen is a long single page containing Gateway controls, diagnostics, Provider creation, Provider rows, connectivity, usage, and timeout settings.
- The most important action is mixed with configuration details and explanatory copy.
- Provider rows contain nested connectivity and usage panels, so scanning several Providers becomes expensive.
- Diagnostics and Gateway timeout controls are useful but compete with the primary launch flow.
- Error feedback is currently a general notice; recovery actions should sit next to the affected Provider or Gateway state.
- The current business state is already sufficient for a richer UI: direct Responses, Gateway passthrough, Chat conversion, connectivity reports, local usage, diagnostics, and Codex launch profiles.

## Proposed Information Architecture

### App shell

- Compact top bar: 3R Codex identity, current environment status, refresh, settings.
- Narrow left navigation: Overview, Providers, Gateway, Diagnostics.
- Main content uses one constrained workspace; avoid nested card stacks.
- Provider details open in a right-side drawer or dedicated detail view instead of expanding every row.

### Overview

- Gateway status strip at the top with one primary action:
  - Gateway required and stopped: `启动 Gateway`.
  - Gateway serving: Provider card uses the circular play icon to launch Gateway plus Codex; stop remains in the sidebar status area.
  - Direct Responses: Provider card uses the same circular play icon to launch the isolated direct Profile.
- Active route section: Provider name, transport mode, endpoint, capability summary, and current status.
- Provider list: compact rows with status, masked key state, local usage summary, and actions; transport mode is selected by the launch plan.
- Recent diagnostics: last few events with severity, route, status, latency, and a link to full diagnostics.

### Provider details

- Identity and endpoint.
- Authentication state and masked credential indicator.
- Connectivity checks: model list and minimal text request.
- Capabilities: Responses, SSE, Chat conversion, tools/MCP support where known.
- Usage: today/week/month local observations, estimated cost, and upstream balance source or explicit unsupported state.
- Actions: test, edit, delete. Joining a Provider to the workspace happens in the workspace selector; launching happens from the workspace.

### Gateway

- Serving state, bind address, port, selected Provider, and client key visibility control.
- Timeout settings in an advanced section, disabled while serving.
- Clear distinction between direct mode and Gateway mode.

## Interaction Rules

- Every visible region has one primary action; destructive actions remain secondary.
- Put recovery beside the failure: `认证失败` should expose `编辑密钥` and `重新检测`.
- Use status labels with both color and text; never rely on color alone.
- Keep API keys masked; reveal is an explicit, temporary action.
- Keep advanced transport and timeout explanations behind disclosure controls.
- Preserve the existing Tauri commands and backend behavior; this redesign changes presentation and interaction grouping only.
- Avoid inert controls: every button must update visible local state or open a real detail surface.
- Use stable dimensions for rows, controls, and status indicators so loading and error text do not shift the layout.

## Visual Direction

- Light neutral desktop console inspired by Cockpit's scan-friendly account cards.
- Charcoal text on cool gray surfaces, teal/green for healthy/current, amber for degraded, coral for errors.
- Subtle borders and shadows, maximum 8px radius for ordinary controls and panels.
- No hero section, marketing copy, decorative gradients, fake metrics, or excessive nested cards.
- Use familiar line icons for refresh, visibility, settings, launch, diagnostics, and delete; add tooltips for icon-only actions.
- Dense enough for repeated Provider management, with generous spacing around the primary action and error recovery.
- The desktop shell owns the viewport: the sidebar stays fixed while only the main workspace scrolls. Scrollbars use a narrow transparent track and a low-contrast thumb that becomes clearer on hover/focus, matching desktop overlay-scrollbar behavior without hiding overflow.

## Required States

- No Provider configured.
- Provider configured but not tested.
- Healthy Provider with current marker.
- Unauthorized Provider with inline recovery.
- Unreachable Provider with retry.
- Gateway stopped and required.
- Gateway serving.
- Direct Responses mode.
- Busy/loading state for every async action.
- Empty diagnostics and populated diagnostics.
- Usage unavailable, local usage available, and upstream balance unsupported.
- Narrow window layout with drawer converted to a full-width detail surface.

## Acceptance Criteria

- A first-time user can identify the current route and next action without reading implementation details.
- Switching Provider, testing connection, starting Gateway, and launching Codex are each reachable within one obvious interaction path.
- Provider failure states show the reason and a recovery action in the same visual context.
- Secrets never appear in status, logs, or screenshots.
- Desktop and narrow-window layouts remain readable without overlap or horizontal scrolling.
- Existing Gateway, Provider, usage, diagnostics, and profile-launch behavior remains intact.

## Implementation Notes

- Added a light console shell with Overview, Providers, Gateway, and Diagnostics views.
- Provider rows now expose only scan-level status, endpoint, masked credential state, local usage, and actions; connectivity and usage details live in a right-side drawer.
- Gateway timeout editing and full diagnostics are available in dedicated views instead of competing with the Overview launch action.
- Existing Tauri commands and secret-storage boundaries are preserved. The frontend never renders the raw Provider API key.
- The empty Provider state has a direct add action, and the main route card exposes one context-aware primary action.
- Theme control now supports light, dark, and system-following modes; the preference is stored locally and applied before the React app renders.
- Gateway navigation, transport modes, Endpoint, diagnostics, usage, credential state, and action buttons now expose concise hover/focus explanations.

## Verification Flow

`Overview -> Providers -> add Provider form -> open Provider detail -> test connection -> Gateway or direct Codex action`

The first implementation slice is verified by TypeScript checking, Vite production build, desktop browser interaction checks, and responsive CSS review for narrow windows. Theme verification covers all three choices, reload persistence, dark rendering, and a 390px viewport. Provider persistence and connectivity must be verified through the packaged Tauri runtime because a browser preview does not provide Tauri's native `invoke` bridge.

## Interaction Refinement Plan (2026-08-06)

## Workspace Utility Placement and Motion (2026-09-12)

### Goal

Keep the two runtime cards as the primary workspace surface. Move secondary entry points closer to the card or section they describe, and give drawers and dialogs a consistent open/close motion.

### Requirements and acceptance

- The isolated App instance center is opened from an icon action on the isolated environment card and keeps its existing drawer content and focus behavior.
- Model channels and history/test records are opened from a compact utility strip above the two environment cards.
- The standalone bottom “附加功能” section is removed from the dashboard.
- The default App warning uses a short two-line reminder and sits directly above the launch actions.
- Drawers enter from right to left with opacity easing; dialogs enter from below with opacity easing.
- Reduced-motion users still receive the same content and state changes without forced movement.
- Desktop and narrow-window layouts have no horizontal overflow, and all icon-only actions retain labels and tooltips.

### Execution

- [x] Move utility entry points into the workspace header/card actions.
- [x] Remove the bottom utility section and obsolete layout rules.
- [x] Add shared drawer/dialog motion tokens and reduced-motion overrides.
- [x] Verify TypeScript, production build, desktop rendering, narrow-window layout, and drawer/dialog open states.

### Verification evidence

- `pnpm check` and `pnpm build` pass after the workspace restructuring.
- In-app Browser confirms the utility strip contains exactly the model-channel and history/test-record entries, the isolated environment card exposes the instance icon, and the bottom utility section is absent.
- Drawer and dialog layers share opacity easing; detail drawers slide in from the right and dialogs slide in from below. `prefers-reduced-motion` disables movement while preserving visibility.

## 主工作区渠道选择重构（2026-09-02）

### 决策

主工作区是唯一的日常操作入口，不再呈现为 Provider 管理列表。用户在此处看到的是：已加入工作区、可供 Codex 选择的模型渠道，以及由这些渠道提供的模型；Provider 的 Endpoint、密钥、检测、用量和删除属于“渠道管理”二级入口。

### 交互合同

- 工作区只保留一个持久化配置；添加或移除渠道、调整模型映射仍写入同一份工作区修订，不创建新的工作区。
- “添加模型渠道”使用选择器，只显示尚未加入工作区的可用 API Provider；已加入渠道显示为可移除的选择结果，而不是另一张全量资源表。
- 工作区主页以“已接入渠道 → 可用模型 → 启动方式”组织信息，并提供 CLI、受管理 App、默认 App 路由三个真实启动入口。
- 渠道管理页不再承担“创建/启动工作区”的职责；它只维护来源。现有 Provider、账户、路由与启动后端合同不变。

### 验收

- 新用户能在工作区首页区分“当前可用”与“尚未加入”的模型渠道，并在不进入渠道管理列表的情况下启动 Codex。
- 多个 Provider 仍可同时加入同一个工作区，Codex 通过模型选择而非“当前 Provider”隐式切换来源。
- 窄窗口下选择器、已选渠道和启动操作不横向溢出。

### Goal

Keep Provider creation and inspection in one consistent right-side drawer workflow, make dark mode a neutral classic dark theme, and explain every unavailable action at the point of interaction.

### Acceptance Criteria

- The `添加 Provider` action opens a right-side drawer; the page content remains stable underneath and the drawer can be closed by its close button or backdrop.
- The add drawer has the same visual anatomy as Provider details, including a clear title, required-field validation, submit/cancel actions, and a narrow-window full-width variant.
- Dark mode uses charcoal/neutral surfaces with restrained blue-gray controls; green is reserved for healthy/current states instead of tinting the whole application.
- Disabled buttons and controls expose a hover/focus explanation on a pointer-safe wrapper, including missing credentials, Gateway-required states, Gateway-running locks, busy/loading state, and incomplete forms.
- Existing Provider/Gateway commands and secret-storage boundaries remain unchanged; the keyring service is selected once from the Tauri application identifier and shared by Provider, Gateway, and the Codex launcher. macOS builds explicitly enable keyring's `apple-native` backend and target the `User` login keychain.

### Execution

- [x] Document the interaction and visual acceptance criteria.
- [x] Refactor Provider creation into the shared drawer surface.
- [x] Add pointer-safe disabled-action hints and reason strings.
- [x] Replace the green-biased dark palette with neutral dark tokens.
- [x] Verify typecheck/build, desktop browser interactions, dark theme, drawer close paths, disabled hints, and narrow-window layout.

## Provider Action Refinement (2026-08-06)

- Provider rows use compact icon actions for connectivity testing, editing, and deletion.
- The duplicate row-level `详情` entry was removed; the edit action is the single row entry for changing Provider metadata.
- Icon-only actions retain native `title`, accessible `aria-label`, and pointer-safe disabled explanations, so compactness does not remove context or recovery guidance.
- Packaged Tauri verification confirmed the three actions render as compact icons, the edit icon opens the right-side edit drawer, and the disabled connectivity icon still exposes its missing-key reason.

## Provider-Owned Gateway Actions (2026-08-06)

- Every Provider row action uses the same circular icon-button family: connectivity, edit, launch, Profile, and delete. Launch uses a play icon instead of text.
- The explicit `设为当前` action is removed. Launching a Provider makes it the recent/default route as part of the launch workflow.
- Gateway timeout fields remain advanced Provider connection protection settings; the launch plan owns the transport mode and the standalone Gateway page is no longer the configuration owner.
- Provider deletion uses an in-app confirmation dialog with explicit cancel and destructive confirmation states; it does not use `window.confirm`.

## Application Instance Lifecycle (2026-08-07)

- The Tauri Single Instance plugin is registered before application setup. A repeated launch exits the new process and focuses the existing main window, so Gateway, SQLite, and credential state remain owned by one process.

## Codex Launch Targets (2026-08-09)

- Every Provider launch action opens a compact dialog that selects `Codex CLI` or `Codex App` and accepts an optional one-time working directory.
- `Codex CLI` starts the interactive CLI in Terminal because it needs visible stdin/stdout. The managed launcher validates the directory and passes it to Codex with `--cd`; it retains the isolated Profile and API Key injection boundary.
- `Codex App` invokes `codex app <directory>` without opening Terminal. It only opens the desktop workspace and uses the App's own login; the API-key Provider Profile and key are not injected, and the UI makes that distinction explicit.
- The managed launcher resolves `CODEX_BIN`, the inherited `PATH`, and common macOS ChatGPT/Codex installation paths before reporting a missing executable. This avoids relying on Finder/Terminal inheriting the same PATH as the development shell.

## Connectivity Loading Feedback (2026-08-06)

- While a Provider connectivity probe is running, its refresh icon changes to a compact animated spinner and its accessible label becomes `正在检测连接`.
- The existing busy lock and disabled-action explanation remain active for the other Provider actions until the probe completes.

## Floating Result Notifications (2026-08-06)

- Provider detection and other operation results now render as a fixed top-right toast, so success or failure feedback no longer pushes the Provider list down.
- Toasts use polite live-region announcements, a close control, and automatic dismissal; error messages stay visible longer than routine status messages.

### Verification Evidence

- `pnpm run check` passed.
- `pnpm run build` passed; Vite produced the production bundle successfully.
- `cargo test --manifest-path src-tauri/Cargo.toml` passed: 30 native tests, plus launcher test target with no tests.
- In-app Browser: opening `添加 Provider` produced a `role="dialog"` right drawer; backdrop/close button and Escape close paths are implemented.
- In-app Browser: an incomplete form rendered a disabled `保存 Provider` button with the pointer-safe wrapper hint `请先填写：API Key。`.
- In-app Browser: dark theme rendered neutral charcoal surfaces (`body rgb(15, 17, 21)`, panel `rgb(25, 29, 36)`) with no horizontal overflow.
- In-app Browser at 390×844: drawer width matched viewport and `scrollWidth === clientWidth === 390`.
- Browser console reported no errors or warnings. Direct Vite preview cannot invoke Tauri commands; this was not treated as CRUD acceptance.
- Packaged Tauri acceptance reproduced the original issue: SQLite metadata persisted while the Provider was reported as unconfigured because the temporary debug app used a different bundle identifier from the hard-coded keyring service.
- The native fix derives one keyring service from `app.config().identifier`, injects the same vault into Provider and Gateway, uses the macOS `User` keychain target, and passes the service to the Codex launcher.
- Packaged Tauri acceptance with a unique `io.3r-gpt.dev` bundle completed: create persisted the masked key, restart retained `configured=true`, edit changed metadata while preserving the key, and deleting the fake test key made `检测连接` disabled with the visible reason `当前 Provider 没有完整配置 API Key，请先重新添加或修复凭据。`.
