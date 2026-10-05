import { formatUnixDate } from "../../lib/formatDate";
import type { components } from "../../lib/serverApi.types";

/** Show a stored API Token hint, which the server writes as `mc-api-xx..yy`. */
export function displayTokenHint(hint: string | null): string {
  return hint?.trim() || "mc-api-..";
}

/** Date a token was created, was last used, or expires: "Never" when there is none. */
export function formatTokenDate(secs: string | null): string {
  return formatUnixDate(secs);
}

/** What an API token is allowed to do, as a readable list. */
export function permissionsLabel(token: { can_import: boolean; can_export: boolean }): string {
  const parts: string[] = [];
  if (token.can_import) parts.push("Import");
  if (token.can_export) parts.push("Export");
  return parts.length > 0 ? parts.join(" / ") : "None";
}

export type ApiTokenItem = components["schemas"]["ApiToken"];

export const thClass = "px-3 py-2 text-left text-[0.75rem] font-bold text-muted";
export const tdClass = "px-3 py-2 text-[0.75rem] text-text align-middle";
export const tdMuted = "px-3 py-2 text-[0.75rem] text-muted align-middle";
