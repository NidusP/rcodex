import {
  AlertTriangle,
  RefreshCw,
} from "lucide-react";
import { LoadingOverlay } from "@mantine/core";
import { type CodexInstallationStatus } from "./codex-control";
import { AppUpdateButton } from "./components/title-bar";
import { IsolatedAppInfo } from "./components/isolated-app";
import {
  useAppState,
  AppStateProvider,
} from "./components/app-state";
import { WorkspaceOverlays } from "./components/model-utility";
import { RuntimeOverlays } from "./components/runtime-panels";
import { HintedButton } from "./components/hinted-button";
import "./App.css";

function CodexInstallationGate({
  status,
  onRetry,
  busy,
}: {
  status: CodexInstallationStatus | null;
  onRetry: () => void;
  busy: boolean;
}) {
  return (
    <section className="installation-gate" role="alert">
      <div className="installation-gate-tools">
        <AppUpdateButton />
      </div>
      <div className="installation-gate-icon">
        <AlertTriangle size={28} aria-hidden="true" />
      </div>
      <p className="eyebrow">CODEX REQUIRED</p>
      <h2>请先安装 Codex</h2>
      <p>
        R Codex 需要本机已安装 Codex
        CLI（命令行程序）才能配置模型渠道、生成入口配置和启动
        App。当前尚未找到可用的 Codex 可执行文件。
      </p>
      {status?.error && <code>{status.error}</code>}
      <HintedButton
        className="primary-action"
        type="button"
        disabled={busy}
        onClick={onRetry}
      >
        <RefreshCw size={15} aria-hidden="true" />
        重新检测
      </HintedButton>
      <small>
        安装完成后点击“重新检测”；不会读取或修改默认 Codex 的会话和认证数据。
      </small>
    </section>
  );
}

function AppView() {
  const {
    openUtility,
    providers,
    accounts,
    presets,
    codexInstallation,
    usageByProvider,
    connectivityByProvider,
    booting,
    testingProviderId,
    uiBusy,
    pendingKey,
    showInfo,
    refresh,
    openDefaultCodex,
    openIsolatedCodexHome,
    requestLaunchPreset,
    testProvider,
    removeProviderFromWorkspace,
    addProviderToWorkspace,
    reorderProvidersInWorkspace,
    toggleWorkspaceModelSelection,
    setDefaultWorkspaceModel,
    setManagedLoginProvider,
    selectManagedWorkspace,
    createManagedInstance,
    renameManagedInstance,
    instances,
    stopInstance,
  } = useAppState();
  return (
    <div className="app-shell">
      <div className="app-main">
        <LoadingOverlay
          visible={
            booting ||
            (import.meta.env.DEV &&
              new URLSearchParams(window.location.search).get("boot") === "1")
          }
          pos="fixed"
          inset={0}
          zIndex={90}
          loaderProps={{ color: "pink", type: "bars" }}
        />
        {codexInstallation && !codexInstallation.installed ? (
          <CodexInstallationGate
            status={codexInstallation}
            onRetry={() => void refresh({ probeInstallation: true })}
            busy={uiBusy}
          />
        ) : (
          <IsolatedAppInfo
            presets={presets}
            providers={providers}
            accounts={accounts}
            pendingKey={pendingKey}
            usageByProvider={usageByProvider}
            connectivityByProvider={connectivityByProvider}
            testingProviderId={testingProviderId}
            onLaunchApp={(preset) =>
              requestLaunchPreset(preset, "managed_app")
            }
            onLaunchCli={(preset) => requestLaunchPreset(preset, "cli")}
            onToggleModel={(preset, providerId, upstreamModelId) =>
              void toggleWorkspaceModelSelection(
                preset,
                providerId,
                upstreamModelId,
              )
            }
            onAddProvider={addProviderToWorkspace}
            onRemoveProvider={removeProviderFromWorkspace}
            onDefaultModelChange={setDefaultWorkspaceModel}
            onLoginProviderChange={setManagedLoginProvider}
            onOpenUtility={(view, preset) => openUtility(view, preset?.id)}
            onOpenInstances={(preset) => openUtility("instances", preset.id)}
            onOpenDefaultCodex={() => void openDefaultCodex()}
            onOpenCodexHome={(preset) =>
              void openIsolatedCodexHome(preset)
            }
            onSyncResult={showInfo}
            onTestProvider={(providerId) => void testProvider(providerId)}
            onReorderChannels={(preset, providerIds) =>
              void reorderProvidersInWorkspace(preset, providerIds)
            }
            onSelectInstance={(preset) => void selectManagedWorkspace(preset)}
            onCreateInstance={(preset) => createManagedInstance(preset)}
            onRenameInstance={(preset, name) =>
              void renameManagedInstance(preset, name)
            }
            instances={instances}
            onStopInstance={(instance) => void stopInstance(instance)}
          />
        )}
        <WorkspaceOverlays />
        <RuntimeOverlays />
      </div>
    </div>
  );
}

export default function App() {
  return (
    <AppStateProvider>
      <AppView />
    </AppStateProvider>
  );
}
