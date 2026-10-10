/** Private request archive contract. Large bodies are fetched in bounded pages. */
export type ArchiveStatus = { enabled: boolean; root: string; requestCount: number; stageCount: number; storedBytes: number };
export type ArchiveRequestSummary = { requestId: string; scopeId: string; conversationKey: string | null; createdAt: number; stages: number; partial: boolean; captureStatus: string; correlationStatus: string };
export type ArchiveStageInfo = { stage: string; observedBytes: number; storedBytes: number; partial: boolean };
export type ArchiveRequestDetail = { request: ArchiveRequestSummary; stages: ArchiveStageInfo[]; metadata: Record<string, unknown> };
export type ArchiveBodyPage = { text: string; offset: number; nextOffset: number; totalBytes: number; truncated: boolean };
