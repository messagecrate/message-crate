/** Client-safe storage types and helpers (no Node/SQLite imports). */

export type VaultImportListItem = {
  id: number;
  source: string;
  tool: string | null;
  mode: string;
  status: string;
  startedAt: string;
  finishedAt: string | null;
  messageCount: number;
  attachmentCount: number;
  bytesUploaded: number;
};

export type TopAttachmentItem = {
  id: number;
  originalName: string | null;
  mimeType: string | null;
  sizeBytes: number;
  /** `null`, with `chatIdentifier`, only when the owner reads another account. */
  conversationId: number | null;
  conversationTitle: string | null;
  chatIdentifier: string | null;
};

export type StorageUsage = {
  totalBytes: number;
  attachmentCount: number;
  topAttachments: TopAttachmentItem[];
};

export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  const digits = value >= 10 || unit === 0 ? 0 : 1;
  return `${value.toFixed(digits)} ${units[unit]}`;
}
