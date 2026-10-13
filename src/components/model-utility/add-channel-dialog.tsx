import { useState } from "react";
import {
  Fieldset,
  NumberInput,
  PasswordInput,
  SimpleGrid,
  TextInput,
} from "@mantine/core";
import { useForm } from "@mantine/form";
import { HintedButton } from "../hinted-button";
import {
  defaultGatewaySettings,
  type GatewayTimeoutSettings,
} from "../../app-model";
import { ControlDrawer } from "./drawer-layout";
import {
  type DraftChannelProbe,
  type ProviderChannelDraft,
} from "./types";
import {
  hiddenCodexRoleModelCount,
  hiddenCodexRoleModelNote,
  imageGenerationModelNote,
  isImageGenerationModelId,
  selectableProviderModels,
} from "../model-labels";

import "./add-channel-dialog.css";

export type { DraftChannelProbe, ProviderChannelDraft };

type ChannelFormValues = {
  name: string;
  baseUrl: string;
  apiKey: string;
  connectSeconds: number | string;
  firstByteSeconds: number | string;
  streamIdleSeconds: number | string;
  nonStreamTotalSeconds: number | string;
};

function requiredText(label: string) {
  return (value: string) => (value.trim() ? null : "请填写" + label);
}

function timeoutNumber(value: number | string) {
  return typeof value === "number" ? value : Number(value);
}

function timeoutError(label: string, min: number, max: number) {
  return (value: number | string) => {
    const number = timeoutNumber(value);
    if (!Number.isInteger(number) || number < min || number > max) {
      return label + "须是 " + min + " 到 " + max + " 的整数秒";
    }
    return null;
  };
}

function gatewaySettingsFrom(values: ChannelFormValues): GatewayTimeoutSettings {
  return {
    providerId: "",
    connectSeconds: timeoutNumber(values.connectSeconds),
    firstByteSeconds: timeoutNumber(values.firstByteSeconds),
    streamIdleSeconds: timeoutNumber(values.streamIdleSeconds),
    nonStreamTotalSeconds: timeoutNumber(values.nonStreamTotalSeconds),
  };
}

/**
 * @component AddChannelDialog
 * @description 全屏新增模型渠道抽屉，叠在模型渠道抽屉之上。字段由 Mantine 表单管理，可先探针再保存。
 */
export function AddChannelDialog({
  busy,
  onProbe,
  onSubmit,
  onClose,
}: {
  busy: boolean;
  onProbe: (baseUrl: string, apiKey: string) => Promise<DraftChannelProbe>;
  onSubmit: (draft: ProviderChannelDraft) => void | Promise<void>;
  onClose: () => void;
}) {
  const defaults = defaultGatewaySettings();
  const [draft, setDraft] = useState<DraftChannelProbe | null>(null);
  const [probing, setProbing] = useState(false);
  const form = useForm<ChannelFormValues>({
    mode: "controlled",
    initialValues: {
      name: "OpenAI",
      baseUrl: "https://api.openai.com/v1",
      apiKey: "",
      connectSeconds: defaults.connectSeconds,
      firstByteSeconds: defaults.firstByteSeconds,
      streamIdleSeconds: defaults.streamIdleSeconds,
      nonStreamTotalSeconds: defaults.nonStreamTotalSeconds,
    },
    validate: {
      name: requiredText("名称"),
      baseUrl: requiredText("Base URL"),
      apiKey: requiredText("API Key"),
      connectSeconds: timeoutError("连接超时", 1, 120),
      firstByteSeconds: timeoutError("首字节超时", 1, 600),
      streamIdleSeconds: timeoutError("流式空闲超时", 1, 1800),
      nonStreamTotalSeconds: (value, values) => {
        const error = timeoutError("非流式总超时", 1, 7200)(value);
        if (error) return error;
        if (timeoutNumber(value) < timeoutNumber(values.firstByteSeconds)) {
          return "非流式总超时不能小于首字节超时。";
        }
        return null;
      },
    },
    onValuesChange: (values, previous) => {
      if (
        values.baseUrl !== previous.baseUrl ||
        values.apiKey !== previous.apiKey
      ) {
        setDraft(null);
      }
    },
  });
  const locked = busy || probing;
  const discoveredModels = draft?.models ?? [];
  const models = selectableProviderModels(discoveredModels);
  const hiddenNote = hiddenCodexRoleModelNote(
    hiddenCodexRoleModelCount(discoveredModels),
  );
  const imageNote = imageGenerationModelNote(
    discoveredModels
      .filter((model) => isImageGenerationModelId(model.upstreamModelId))
      .map((model) => model.displayName || model.upstreamModelId),
  );
  const protocol =
    draft?.wireApi === "responses"
      ? "Responses"
      : draft?.wireApi === "chat_completions"
        ? "Chat Completions"
        : null;
  async function probe() {
    const nameError = requiredText("名称")(form.values.name);
    const urlError = requiredText("Base URL")(form.values.baseUrl);
    const keyError = requiredText("API Key")(form.values.apiKey);
    form.setFieldError("name", nameError);
    form.setFieldError("baseUrl", urlError);
    form.setFieldError("apiKey", keyError);
    if (nameError || urlError || keyError) return;
    setProbing(true);
    try {
      setDraft(await onProbe(form.values.baseUrl, form.values.apiKey));
    } catch {
      setDraft(null);
    } finally {
      setProbing(false);
    }
  }
  return (
    <ControlDrawer
      opened
      onClose={onClose}
      title="添加模型渠道"
      size="100%"
      zIndex={80}
      closeDisabled={locked}
      withBackButton
    >
      <p className="drawer-intro">
        保存一条上游 API 模型渠道。API Key 只写入系统凭据库。可以先探针测试，成功后显示协议和可用模型；点保存才写入结果。要更换已有渠道，请删除后重新添加。
      </p>
      <form
        className="provider-form drawer-form channel-mantine-form"
        onSubmit={form.onSubmit((values) => {
          void onSubmit({
            name: values.name,
            baseUrl: values.baseUrl,
            apiKey: values.apiKey,
            gatewaySettings: gatewaySettingsFrom(values),
            ...(draft ? { probeTicketId: draft.probeTicketId } : {}),
          });
        })}
      >
        <SimpleGrid cols={{ base: 1, sm: 2 }} spacing="sm">
          <TextInput
            label="名称"
            placeholder="OpenAI"
            disabled={locked}
            {...form.getInputProps("name")}
          />
          <TextInput
            label="Base URL"
            placeholder="https://api.openai.com/v1"
            disabled={locked}
            {...form.getInputProps("baseUrl")}
          />
        </SimpleGrid>
        <PasswordInput
          label="API Key"
          placeholder="仅写入系统凭据库"
          title="API Key 只写入系统凭据库。"
          autoComplete="off"
          disabled={locked}
          {...form.getInputProps("apiKey")}
        />
        <Fieldset legend="Gateway 超时策略" disabled={locked}>
          <p>
            超时用于本机 Gateway 的连接保护。探针使用单独的 15 秒超时。
          </p>
          <SimpleGrid cols={{ base: 1, sm: 2, md: 4 }} spacing="sm">
            <NumberInput
              label="连接（秒）"
              min={1}
              max={120}
              clampBehavior="strict"
              allowDecimal={false}
              disabled={locked}
              {...form.getInputProps("connectSeconds")}
            />
            <NumberInput
              label="首字节（秒）"
              min={1}
              max={600}
              clampBehavior="strict"
              allowDecimal={false}
              disabled={locked}
              {...form.getInputProps("firstByteSeconds")}
            />
            <NumberInput
              label="流式空闲（秒）"
              min={1}
              max={1800}
              clampBehavior="strict"
              allowDecimal={false}
              disabled={locked}
              {...form.getInputProps("streamIdleSeconds")}
            />
            <NumberInput
              label="非流式总计（秒）"
              min={1}
              max={7200}
              clampBehavior="strict"
              allowDecimal={false}
              disabled={locked}
              {...form.getInputProps("nonStreamTotalSeconds")}
            />
          </SimpleGrid>
        </Fieldset>
        <div className="channel-probe-result" aria-live="polite">
          {!draft && !probing && (
            <p>
              填写后可以先探测。探测会读取模型列表，并可能发送一次最小文本请求，上游可能计费。
            </p>
          )}
          {probing && <p>正在探测…</p>}
          {draft && protocol && (
            <>
              <strong>探测成功 · 协议 {protocol}</strong>
              <div>
                {models.map((model) => (
                  <span key={model.upstreamModelId} title={model.upstreamModelId}>
                    {model.displayName || model.upstreamModelId}
                  </span>
                ))}
              </div>
              {hiddenNote ? <p>{hiddenNote}</p> : null}
              {imageNote ? <p>{imageNote}</p> : null}
            </>
          )}
          {draft && !protocol && draft.modelsCheck === "ready" && (
            <>
              <strong>协议未确认</strong>
              <div>
                {models.map((model) => (
                  <span key={model.upstreamModelId}>
                    {model.displayName || model.upstreamModelId}
                  </span>
                ))}
              </div>
              {hiddenNote ? <p>{hiddenNote}</p> : null}
              {imageNote ? <p>{imageNote}</p> : null}
            </>
          )}
          {draft && !protocol && draft.modelsCheck !== "ready" && (
            <p>
              {"探测未成功" +
                (draft.errorCode ? " · " + draft.errorCode : "") +
                "。"}
            </p>
          )}
        </div>
        <div className="drawer-actions form-actions">
          <HintedButton
            className="secondary-action"
            type="button"
            title="读取模型列表并判断上游协议。可能产生一次最小文本请求费用。"
            disabledReason={probing ? "正在探测渠道。" : undefined}
            disabled={locked}
            onClick={() => void probe()}
          >
            {probing ? "正在探测…" : "探针测试"}
          </HintedButton>
          <HintedButton
            className="primary-action"
            type="submit"
            title="保存模型渠道、Gateway 设置并将 API Key 写入系统凭据库。"
            disabledReason={locked ? "正在处理模型渠道，请稍候。" : undefined}
            disabled={locked}
          >
            保存模型渠道
          </HintedButton>
          <HintedButton
            className="text-button"
            type="button"
            onClick={onClose}
            disabled={locked}
          >
            取消
          </HintedButton>
        </div>
      </form>
    </ControlDrawer>
  );
}

/** @see AddChannelDialog */
export const AddProviderDialog = AddChannelDialog;
