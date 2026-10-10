import { ChannelDrawer } from "./channel-drawer";
import { ChannelOverlays } from "./channel-workspace";
import { DiagnosticsDrawer } from "./diagnostics-drawer";
import { InstanceDrawer } from "./instance-drawer";

/**
 * @component WorkspaceOverlays
 * @description 工作区叠层根。挂载模型渠道、实例、诊断抽屉以及渠道详情/新增/删除确认。
 */
export function WorkspaceOverlays() {
  return (
    <>
      <ChannelDrawer />
      <InstanceDrawer />
      <DiagnosticsDrawer />
      <ChannelOverlays />
    </>
  );
}
