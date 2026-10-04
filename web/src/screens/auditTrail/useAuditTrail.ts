import { keepPreviousData } from "@tanstack/react-query";
import { useState } from "react";
import { apiErrorMessage } from "../../lib/apiErrorMessage";
import { keys } from "../../lib/queryKeys";
import { useRouteQuery } from "../../lib/routeQuery";
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
 * logged-in account's own, one account the owner has opened, or a deleted
 * account's, which the owner picks by its old username.
 */
export type AuditTrailOf =
  | { kind: "all" }
  | { kind: "own" }
  | { kind: "account"; id: number }
  | { kind: "deleted"; username: string };

/** The cache key's word for whose trail is read. */
function whoseKey(of: AuditTrailOf) {
  switch (of.kind) {
    case "account":
      return of.id;
    case "deleted":
      return `deleted:${of.username}` as const;
    default:
      return of.kind;
  }
}

/** Read one page of the Audit Trail `of` names. */
function readAuditTrail(of: AuditTrailOf, params: AuditTrailParams, signal: AbortSignal) {
  switch (of.kind) {
    case "all":
      return listAuditTrail(params, { signal });
    case "deleted":
      return listAuditTrail({ ...params, username: of.username }, { signal });
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
  const whose = whoseKey(of);
  const [paging, setPaging] = useState<{ whose: typeof whose; page: number }>({ whose, page: 0 });
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
 * The usernames of the deleted accounts whose entries the Audit Trail keeps,
 * A to Z, for the owner's account picker. Empty while they load or when the
 * read fails: the picker still offers every live account.
 */
export function useDeletedAccountUsernames(): string[] {
  const { data } = useRouteQuery(keys.auditTrail.deletedAccounts, (signal) =>
    listDeletedAccounts({ signal }),
  );
  return data?.map((account) => account.username) ?? [];
}
