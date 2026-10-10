import { findImportSource } from "./importSources";

/** Import Run / messages.source slug for a desktop Import method id; an unknown id is kept. */
export function sourceForMethod(source: string): string {
  return findImportSource(source)?.id ?? source;
}

/** Body for POST /v1/imports. Maps method ids; leaves other sources as-is. */
export function importRunCreateBody(formSource: string): {
  source: string;
  tool: "message-crate";
  mode: "append";
} {
  return {
    source: sourceForMethod(formSource),
    tool: "message-crate",
    mode: "append",
  };
}
