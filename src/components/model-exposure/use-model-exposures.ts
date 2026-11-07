import { useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { toCommandError } from "../../codex-control";
import type { ModelExposureSummary, ModelExposuresByProvider } from "../../model-exposure";

/**
 * @function useModelExposures
 * @description 读取透出状态，执行行级透出保存与显式图片检测，失败时关闭新增入口。
 */
export function useModelExposures(
  onError: (error: unknown, prefix: string) => void,
) {
  const [exposuresByProvider, setExposuresByProvider] = useState<ModelExposuresByProvider>({});
  const [exposureErrorsByProvider, setExposureErrorsByProvider] = useState<Record<string, string>>({});
  const [exposurePending, setExposurePending] = useState<Set<string>>(() => new Set());
  const [modelExposureErrors, setModelExposureErrors] = useState<Record<string, string>>({});
  const active = useRef(new Set<string>());
  const readVersions = useRef(new Map<string, number>());

  async function refreshProviderExposures(providerId: string) {
    const version = (readVersions.current.get(providerId) ?? 0) + 1;
    readVersions.current.set(providerId, version);
    try {
      const models = await invoke<ModelExposureSummary[]>("list_provider_model_exposures", { providerId });
      if (!Array.isArray(models)) throw new Error("invalid_model_exposure_response");
      if (readVersions.current.get(providerId) !== version) return;
      setExposuresByProvider((current) => ({ ...current, [providerId]: models }));
      setExposureErrorsByProvider((current) => {
        const next = { ...current };
        delete next[providerId];
        return next;
      });
    } catch {
      if (readVersions.current.get(providerId) !== version) return;
      setExposuresByProvider((current) => ({ ...current, [providerId]: [] }));
      setExposureErrorsByProvider((current) => ({ ...current, [providerId]: "模型透出状态读取失败，请重试。" }));
    }
  }

  async function refreshModelExposures(providerIds: string[]) {
    await Promise.all(providerIds.map(refreshProviderExposures));
  }

  async function changeModelExposure(providerId: string, upstreamModelId: string, exposed: boolean) {
    return runModelCommand(providerId, upstreamModelId, "set_provider_model_exposure", { exposed });
  }

  async function refreshModelImageProbe(providerId: string, upstreamModelId: string) {
    return runModelCommand(providerId, upstreamModelId, "refresh_model_image_probe");
  }

  async function runModelCommand(
    providerId: string,
    upstreamModelId: string,
    command: string,
    extra: { exposed?: boolean } = {},
  ) {
    const key = JSON.stringify([providerId, upstreamModelId]);
    if (active.current.has(key)) return;
    active.current.add(key);
    setExposurePending(new Set(active.current));
    setModelExposureErrors((current) => {
      const next = { ...current };
      delete next[key];
      return next;
    });
    if (extra.exposed !== undefined) {
      setExposuresByProvider((current) => ({
        ...current,
        [providerId]: (current[providerId] ?? []).map((model) =>
          model.upstreamModelId === upstreamModelId ? { ...model, exposed: extra.exposed! } : model,
        ),
      }));
    }
    try {
      const saved = await invoke<ModelExposureSummary>(command, { providerId, upstreamModelId, ...extra });
      // A newer read may have happened while awaiting the image request. Read
      // the current revision rather than installing a stale command result.
      if (saved.providerId === providerId) await refreshProviderExposures(providerId);
    } catch (error) {
      setModelExposureErrors((current) => ({ ...current, [key]: toCommandError(error).message }));
      onError(error, command === "set_provider_model_exposure" ? "无法修改模型透出" : "图片检测未完成");
      await refreshProviderExposures(providerId);
    } finally {
      active.current.delete(key);
      setExposurePending(new Set(active.current));
    }
  }

  return {
    exposuresByProvider,
    exposureErrorsByProvider,
    refreshModelExposures,
    refreshProviderExposures,
    changeModelExposure,
    refreshModelImageProbe,
    modelExposurePending: (providerId: string, modelId: string) => exposurePending.has(JSON.stringify([providerId, modelId])),
    modelExposureError: (providerId: string, modelId: string) => modelExposureErrors[JSON.stringify([providerId, modelId])],
  };
}
