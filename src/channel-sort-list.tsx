import { useState, type ReactNode } from "react";
import { DndContext, DragOverlay, KeyboardSensor, PointerSensor, closestCenter, useSensor, useSensors, type DragEndEvent, type DragStartEvent } from "@dnd-kit/core";
import { restrictToVerticalAxis } from "@dnd-kit/modifiers";
import { SortableContext, arrayMove, sortableKeyboardCoordinates, useSortable, verticalListSortingStrategy } from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
import { GripVertical } from "lucide-react";
import type { WorkspaceChannelRow } from "./workspace-surface";
import { HintedButton } from "./components/hinted-button";
import { Checkbox, Popover } from "@mantine/core";
import { hiddenCodexRoleModelNote } from "./components/model-labels";
import "./channel-sort-list.css";

type RowActions = {
  configureDisabled: boolean;
  configureReason?: string;
  testingChannelId: string | null;
  onTestChannel: (providerId: string) => void;
  onToggleModel: (providerId: string, upstreamModelId: string) => void;
  onRemoveChannel: (providerId: string) => void;
};

function imageModelNote(row: WorkspaceChannelRow) {
  if (row.imageModelCount <= 0) return null;
  if (row.imageModelLabels.length === 0) {
    return "另有 " + row.imageModelCount + " 个图像生成模型，用于单独出图，不会显示在工作区勾选中。";
  }
  const shown = row.imageModelLabels.join("、");
  const extra = row.imageModelCount > row.imageModelLabels.length
    ? " 等 " + row.imageModelCount + " 个模型"
    : "";
  return shown + extra + " 是图像生成模型，用于单独出图，不会显示在工作区勾选中。";
}

function ChannelModelChoices({ row, actions }: { row: WorkspaceChannelRow; actions: RowActions }) {
  const testing = actions.testingChannelId === row.id;
  const disabled = actions.configureDisabled || testing;
  if (!row.probed || row.modelChoices.length === 0) {
    return (
      <div className="channel-row-models">
        <span className="channel-row-model-empty">
          {row.probed ? "这次探测没有可勾选的对话模型。" : "尚未探测"}
        </span>
        <HintedButton className="text-button" type="button" disabled={testing} onClick={() => actions.onTestChannel(row.id)}>
          刷新模型目录
        </HintedButton>
      </div>
    );
  }
  return (
    <div className="channel-row-models" role="group" aria-label={row.label + " 的模型"}>
      {row.modelChoices.map((choice) => (
        <Checkbox
          key={choice.upstreamModelId}
          checked={choice.selected}
          disabled={disabled}
          onChange={() => actions.onToggleModel(row.id, choice.upstreamModelId)}
          label={<span title={choice.detail}>{choice.label}</span>}
        />
      ))}
    </div>
  );
  }
function RemoveChannelButton({ row, actions }: { row: WorkspaceChannelRow; actions: RowActions }) {
  const [opened, setOpened] = useState(false);
  return (
    <Popover
      opened={opened && !actions.configureDisabled}
      onChange={setOpened}
      position="top"
      withArrow
      shadow="md"
      withinPortal
      radius="md"
      zIndex={420}
    >
      <Popover.Target>
        <span className="channel-remove-target">
          <HintedButton
            className="text-button"
            type="button"
            disabled={actions.configureDisabled}
            disabledReason={actions.configureReason}
            aria-haspopup="dialog"
            aria-expanded={opened}
            onClick={() => {
              if (!actions.configureDisabled) setOpened((value) => !value);
            }}
          >
            移除
          </HintedButton>
        </span>
      </Popover.Target>
      <Popover.Dropdown className="channel-remove-popover">
        <p>从当前隔离实例移出「{row.label}」？</p>
        <div>
          <HintedButton className="text-button" type="button" onClick={() => setOpened(false)}>
            取消
          </HintedButton>
          <HintedButton
            className="text-button"
            type="button"
            onClick={() => {
              setOpened(false);
              actions.onRemoveChannel(row.id);
            }}
          >
            移除
          </HintedButton>
        </div>
      </Popover.Dropdown>
    </Popover>
  );
}

function ChannelRowBody({
  row,
  actions,
  handle,
}: {
  row: WorkspaceChannelRow;
  actions: RowActions;
  handle: ReactNode;
}) {
  const hiddenNote = hiddenCodexRoleModelNote(row.hiddenRoleModelCount);
  const imageNote = imageModelNote(row);
  const testing = actions.testingChannelId === row.id;
  return (
    <>
      <div className="channel-row-main">
        <div className="channel-row-title">
          <strong>{row.label}</strong>
          {row.isLogin && <span className="current-mark">API 登录</span>}
          <span className={"status-badge " + row.statusTone}>
            <i aria-hidden="true" />
            {row.statusLabel}
          </span>
        </div>
        <small>
          {row.host} · {row.protocolLabel} · 探测到 {row.discoveredCount} 个 · {row.usageLabel}
        </small>
        <ChannelModelChoices row={row} actions={actions} />
        {hiddenNote ? <small className="channel-row-model-note">{hiddenNote}</small> : null}
        {imageNote ? <small className="channel-row-model-note">{imageNote}</small> : null}
        {row.selectedImageModels.length > 0 ? (
          <div className="channel-row-models">
            {row.selectedImageModels.map((model) => (
              <HintedButton
                key={model.upstreamModelId}
                className="text-button"
                type="button"
                disabled={actions.configureDisabled || testing}
                onClick={() => actions.onToggleModel(row.id, model.upstreamModelId)}
              >
                {"移除 " + model.label}
              </HintedButton>
            ))}
          </div>
        ) : null}
      </div>
      <div className="channel-row-actions">
        {row.recover && (
          <HintedButton className="text-button" type="button" disabled={testing} onClick={() => actions.onTestChannel(row.id)}>
            {testing ? "检测中" : "重新检测"}
          </HintedButton>
        )}
        <RemoveChannelButton row={row} actions={actions} />
      </div>
      <div className="channel-drag-slot">{handle}</div>
    </>
  );
}

function SortableChannelRow({ row, actions }: { row: WorkspaceChannelRow; actions: RowActions }) {
  const { attributes, listeners, setNodeRef, transform, transition, isDragging } = useSortable({
    id: row.id,
    disabled: actions.configureDisabled,
  });
  const reduceMotion = typeof window !== "undefined" && window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  const draggingClass = isDragging ? "channel-row is-dragging" : "channel-row";
  return (
    <div
      ref={setNodeRef}
      style={{ transform: CSS.Transform.toString(transform), transition: reduceMotion ? undefined : transition }}
      className={draggingClass}
    >
      <ChannelRowBody
        row={row}
        actions={actions}
        handle={
          <HintedButton
            className="channel-drag"
            type="button"
            disabled={actions.configureDisabled}
            disabledReason={actions.configureReason}
            aria-label={"拖拽排序 " + row.label}
            title={actions.configureDisabled ? actions.configureReason ?? "当前不能调整顺序。" : "拖拽调整渠道顺序。空格拾起，方向键移动，再按空格放下。模型目录会按这个顺序生成"}
            {...attributes}
            {...listeners}
          >
            <GripVertical size={16} aria-hidden="true" />
          </HintedButton>
        }
      />
    </div>
  );
}

/**
 * @component ChannelRowList
 * @description 可拖拽排序的接入渠道列表。模型在渠道行上直接勾选。
 */
export function ChannelRowList({
  rows,
  actions,
  onReorder,
}: {
  rows: WorkspaceChannelRow[];
  actions: RowActions;
  onReorder: (providerIds: string[]) => void;
}) {
  const [activeId, setActiveId] = useState<string | null>(null);
  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 6 } }),
    useSensor(KeyboardSensor, { coordinateGetter: sortableKeyboardCoordinates }),
  );
  const active = rows.find((row) => row.id === activeId) ?? null;
  function onDragStart(event: DragStartEvent) { setActiveId(String(event.active.id)); }
  function onDragEnd(event: DragEndEvent) {
    setActiveId(null);
    if (actions.configureDisabled || !event.over || event.active.id === event.over.id) return;
    const ids = rows.map((row) => row.id);
    const from = ids.indexOf(String(event.active.id));
    const to = ids.indexOf(String(event.over.id));
    if (from < 0 || to < 0) return;
    onReorder(arrayMove(ids, from, to));
  }
  return (
    <DndContext sensors={sensors} collisionDetection={closestCenter} modifiers={[restrictToVerticalAxis]} onDragStart={onDragStart} onDragCancel={() => setActiveId(null)} onDragEnd={onDragEnd}>
      <SortableContext items={rows.map((row) => row.id)} strategy={verticalListSortingStrategy}>
        <div className="channel-rows">{rows.map((row) => <SortableChannelRow key={row.id} row={row} actions={actions} />)}</div>
      </SortableContext>
      <DragOverlay>
        {active && (
          <div className="channel-row channel-row-overlay">
            <ChannelRowBody row={active} actions={actions} handle={<span className="channel-drag" aria-hidden="true"><GripVertical size={16} /></span>} />
          </div>
        )}
      </DragOverlay>
    </DndContext>
  );
}
