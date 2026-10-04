import { keepPreviousData } from "@tanstack/react-query";
import { useState } from "react";
import { apiErrorMessage } from "../../lib/apiErrorMessage";
import { type AuditTrailKey, keys } from "../../lib/queryKeys";
import { useRouteQuery } from "../../lib/routeQuery";
import type { DeletedAccount } from "../../lib/serverApi";
import {
  type AuditTrailParams,
  listAccountAuditTrail,
  listAuditTrail,
  listDeletedAccounts,
} from "../../lib/serverApi";

/** Entries the Audit Trail shows per page. */
export const AUDIT_TRAIL_PAGE_SIZE = 50;

/**
 * Whose Audit Trail to read: every account's (the owner's view), the
 * logged-in account's own, one account the owner has opened, or one deleted
 * account's, by its id from the deleted-accounts list.
 */
export type AuditTrailOf =
  | { kind: "all" }
  | { kind: "own" }
  | { kind: "account"; id: number }
  | { kind: "deleted"; id: number };

/** The key that names `of`: the cache key's word for it, and the owner's account picker's. */
export function auditTrailKey(of: AuditTrailOf): AuditTrailKey {
  switch (of.kind) {
    case "account":
    case "deleted":
      return `${of.kind}:${of.id}`;
    default:
      return of.kind;
  }
}

/**
 * Whose trail a key from `auditTrailKey` names. A picker hands its key back
 * as a plain string, so anything else reads as every account's.
 */
export function auditTrailOf(key: string): AuditTrailOf {
  if (key === "own") return { kind: "own" };
  const [kind, id] = key.split(":");
  if ((kind === "account" || kind === "deleted") && id !== undefined && /^\d+$/.test(id)) {
    return { kind, id: Number(id) };
  }
  return { kind: "all" };
}

/** Read one page of the Audit Trail `of` names. */
function readAuditTrail(of: AuditTrailOf, params: AuditTrailParams, signal: AbortSignal) {
  switch (of.kind) {
    case "all":
      return listAuditTrail(params, { signal });
    case "deleted":
      return listAuditTrail({ ...params, deleted_account_id: of.id }, { signal });
    case "own":
      return listAccountAuditTrail(params, { signal });
    case "account":
      return listAccountAuditTrail(params, { signal }, of.id);
  }
}

/**
 * One page of an Audit Trail, newest first, and the page control's state.
 * The page on screen stays up while the next one loads, so the table does
 * not blank between pages. Changing whose trail is read starts at page one.
 */
export function useAuditTrail(of: AuditTrailOf) {
  const whose = auditTrailKey(of);
  const [paging, setPaging] = useState({ whose, page: 0 });
  const page = paging.whose === whose ? paging.page : 0;
  const params = { limit: AUDIT_TRAIL_PAGE_SIZE, offset: page * AUDIT_TRAIL_PAGE_SIZE };

  const { data, isPending, error } = useRouteQuery(
    keys.auditTrail.page(whose, page),
    (signal) => readAuditTrail(of, params, signal),
    { placeholderData: keepPreviousData },
  );

  return {
    entries: data?.items ?? [],
    total: data?.total ?? 0,
    page,
    setPage: (next: number) => setPaging({ whose, page: next }),
    loading: isPending,
    error: error ? apiErrorMessage(error, "Could not load the Audit Trail.") : "",
  };
}

/**
 * The deleted accounts whose entries the Audit Trail keeps, by username A to
 * Z, for the owner's account picker. Empty while they load or when the read
 * fails: the picker still offers every live account.
 */
export function useDeletedAccounts(): DeletedAccount[] {
  const { data } = useRouteQuery(keys.auditTrail.deletedAccounts, (signal) =>
    listDeletedAccounts({ signal }),
  );
  return data ?? [];
}
