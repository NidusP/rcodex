import { CirclePlus } from "lucide-react";
import { HintedButton } from "../hinted-button";
import { useAppState } from "../app-state";
import { ProviderList } from "./channel-workspace";
import { ControlDrawer } from "./drawer-layout";
import "./channel-drawer.css";

/**
 * @component ChannelDrawer
 * @description 全屏模型渠道抽屉。列出已保存的 API Key 渠道，并提供添加入口。
 */
export function ChannelDrawer() {
  const {
    setUtilityDrawer,
    providers,
    usageByProvider,
    connectivityByProvider,
    testingProviderId,
    setSelectedProviderModelCatalogId,
    uiBusy,
    pendingKey,
    testProvider,
    requestDeleteProvider,
    renameProvider,
    openAddProvider,
    utilityDrawer,
  } = useAppState();
  return (
    <ControlDrawer
      opened={utilityDrawer === "credentials"}
      onClose={() => setUtilityDrawer(null)}
      title="模型渠道"
      size="100%"
      closeDisabled={uiBusy}
      withBackButton
    >
      <div className="channel-drawer-lead">
        <p className="drawer-intro">
          管理 R Codex 工作区使用的 API Key 模型渠道；这些凭据不会写入用户 Codex。
        </p>
        <div className="drawer-toolbar">
          <HintedButton
            className="icon-plain channel-add-circle"
            type="button"
            size="md"
            title="添加模型渠道"
            aria-label="添加模型渠道"
            disabledReason="正在读取状态，请稍候。"
            disabled={uiBusy}
            onClick={openAddProvider}
          >
            <CirclePlus size={22} aria-hidden="true" />
          </HintedButton>
        </div>
      </div>
      <ProviderList
        providers={providers}
        usageByProvider={usageByProvider}
        connectivityByProvider={connectivityByProvider}
        busy={uiBusy}
        pendingKey={pendingKey}
        testingProviderId={testingProviderId}
        onTest={(providerId) => testProvider(providerId)}
        onDelete={requestDeleteProvider}
        onViewModels={setSelectedProviderModelCatalogId}
        onRename={renameProvider}
        onRequestAdd={openAddProvider}
      />
    </ControlDrawer>
  );
}
