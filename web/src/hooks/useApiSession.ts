import { useEffect } from "react";
import { type ApiSession, applyApiSession } from "../lib/apiSession";

/** Keep `api.ts` on `session` whenever any of its fields changes. */
export function useApiSession({ serverUrl, token, accountId }: ApiSession): void {
  useEffect(() => {
    applyApiSession({ serverUrl, token, accountId });
  }, [serverUrl, token, accountId]);
}
