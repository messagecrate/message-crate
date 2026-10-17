import { useCallback } from "react";
import { stepsFor } from "../screens/import/importProgressState";
import {
  type ImportRunState,
  type ImportRunStore,
  importRunStore,
  initialImportRunState,
} from "../screens/import/importRunStore";
import { useAuth } from "./useAuth";
import { useExternalStore } from "./useExternalStore";

/** The fresh form, as an account that has no run in the store reads it. */
const FRESH_RUN = initialImportRunState(stepsFor("copy"));

/**
 * The run in the store when `accountId` started it, and the fresh form when
 * another account did.
 */
function importRunFor(state: ImportRunState, accountId: number | null): ImportRunState {
  return state.accountId === accountId ? state : FRESH_RUN;
}

/**
 * The logged-in account's run in `store` as it stands, re-rendering the
 * caller whenever any of it changes.
 */
export function useImportRunState(store: ImportRunStore = importRunStore): ImportRunState {
  const accountId = useAuth().accountId ?? null;
  const select = useCallback(
    (state: ImportRunState) => importRunFor(state, accountId),
    [accountId],
  );
  return useExternalStore(store, select);
}
