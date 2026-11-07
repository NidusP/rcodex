/** 渠道模型的显式透出偏好与当前配置下的图片检测结果。 */
export type ModelExposureSummary = {
  providerId: string;
  upstreamModelId: string;
  exposed: boolean;
  imageProbeState: "unknown" | "supported" | "unsupported" | "failed";
  imageProbeCheckedAt: number | null;
  imageProbeErrorCode: string | null;
  referencedBy: string[];
};

export type ModelExposuresByProvider = Record<string, ModelExposureSummary[]>;

/** 空间只能添加已经由用户透出的普通对话模型。 */
export function isModelExposed(
  exposures: ModelExposuresByProvider,
  providerId: string,
  upstreamModelId: string,
) {
  return exposures[providerId]?.some(
    (model) => model.upstreamModelId === upstreamModelId && model.exposed,
  ) ?? false;
}

/** 图片检测结果文案；网络失败不表示模型不支持图片。 */
export function imageProbeLabel(state: ModelExposureSummary["imageProbeState"]) {
  return {
    unknown: "图片未检测",
    supported: "支持图片",
    unsupported: "不支持图片",
    failed: "图片检测失败",
  }[state];
}

/** 将固定的探针错误分类转成可读提示，原始正文不进入控制面。 */
export function imageProbeErrorHint(code: string | null) {
  if (!code) return "";
  if (/timeout/.test(code)) return "请求超时，可以手动刷新检测。";
  if (/unauthorized|authentication|credential|forbidden/.test(code)) return "渠道认证失败，请检查 API Key。";
  if (/rate|429/.test(code)) return "上游限制请求频率，请稍后手动刷新。";
  if (/unsupported/.test(code)) return "上游明确表示此模型不支持图片。";
  if (/answer|recogn|semantic|mismatch/.test(code)) return "回复未能正确识别测试图片，可以手动重测。";
  if (/network|connect|transport/.test(code)) return "网络连接失败，可以稍后手动刷新。";
  return "此次未能确认图片能力，可以手动刷新检测。";
}
