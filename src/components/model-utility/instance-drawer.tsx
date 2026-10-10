import { useAppState } from "../app-state";
import { RuntimeInstanceList } from "../runtime-panels";
import { ControlDrawer } from "./drawer-layout";
import "./instance-drawer.css";

/**
 * @component InstanceDrawer
 * @description 当前实例详情。只展示正在查看的隔离实例自己的启动记录。
 */
export function InstanceDrawer() {
  const {
    utilityDrawer,
    setUtilityDrawer,
    gateway,
    providers,
    accounts,
    presets,
    focusedInstanceIds,
    setInstanceDeleteCandidate,
    setInstanceClearCandidate,
    instanceDiagnosticsById,
    instanceSummariesById,
    instanceDiagnosticsLoading,
    uiBusy,
    pendingKey,
    loadInstanceDiagnostics,
    stopInstance,
    restartInstance,
    reconcileInstance,
    visibleManagedInstances,
    scopedPresetId,
  } = useAppState();
  const preset = presets.find((item) => item.id === scopedPresetId);
  const instances = visibleManagedInstances.filter(
    (instance) => instance.launchPresetId === scopedPresetId,
  );
  return (
    <ControlDrawer
      opened={utilityDrawer === "instances"}
      onClose={() => setUtilityDrawer(null)}
      title="实例详情"
      size="min(960px, 100%)"
      closeDisabled={uiBusy}
    >
      <p className="drawer-intro">
        {preset
          ? "只显示当前实例“" + preset.name + "”的启动详情。其他实例不会出现在这里。"
          : "请先选择一个实例。"}
      </p>
        <RuntimeInstanceList
          instances={instances}
          presets={presets}
          providers={providers}
          accounts={accounts}
          gateway={gateway}
          focusIds={focusedInstanceIds}
          pendingKey={pendingKey}
          onStop={stopInstance}
          onRestart={restartInstance}
          onReconcile={reconcileInstance}
          onDelete={setInstanceDeleteCandidate}
          diagnosticsById={instanceDiagnosticsById}
          summariesById={instanceSummariesById}
          loadingId={instanceDiagnosticsLoading}
          onLoadDiagnostics={(instanceId) => void loadInstanceDiagnostics(instanceId)}
          onClearDiagnostics={setInstanceClearCandidate}
        />
    </ControlDrawer>
  );
}
