import type { UseMutationResult } from "@tanstack/react-query";
import { useClearContactHistory } from "../../hooks/useContactHistory";
import { useRouteMutation } from "../../hooks/useRouteQuery";
import {
  deleteContact,
  deleteConversation,
  emptyTrash,
  restoreContact,
  restoreConversation,
} from "../../lib/serverApi";

/**
 * The Trash screen's writes. Trash is a soft marker on two different nouns —
 * conversations and contacts — set and cleared by four idempotent routes, and
 * the one door to permanent deletion: Delete on a trashed row, and Empty
 * Trash for everything in it. Moving a row to the Trash is
 * `useTrashConversation` (`screens/message/`) and `useTrashContact`
 * (`components/`), beside the one screen or component that does it.
 *
 * Unlike Contact Groups and Message Tags this is not a `nameCollection`:
 * there is no name, no membership, and nothing to look an id up by — the
 * caller already has the conversation or contact id. Each one is a plain
 * mutation over one server route.
 *
 * The three deletes for good clear the Contacts list's Undo and Redo stack
 * once they settle, because an undo restores from the Trash and what they
 * deleted is no longer there (#3234). Restore leaves the stack alone.
 */

export function useRestoreConversation(): UseMutationResult<void, Error, number> {
  return useRouteMutation({ mutationFn: restoreConversation });
}

/** Permanently delete a trashed conversation. */
export function useDeleteConversation(): UseMutationResult<void, Error, number> {
  const clearHistory = useClearContactHistory();
  return useRouteMutation({ mutationFn: deleteConversation, onSettled: clearHistory });
}

export function useRestoreContact(): UseMutationResult<void, Error, string | number> {
  return useRouteMutation({ mutationFn: restoreContact });
}

/**
 * Delete a trashed contact: the name and details go and the contact becomes
 * Unknown again, its conversations untouched.
 */
export function useDeleteContact(): UseMutationResult<void, Error, string | number> {
  const clearHistory = useClearContactHistory();
  return useRouteMutation({ mutationFn: deleteContact, onSettled: clearHistory });
}

/**
 * Empty the trash: what `useDeleteConversation` does to every trashed
 * conversation and `useDeleteContact` to every trashed contact, in one server
 * call.
 */
export function useEmptyTrash(): UseMutationResult<void, Error, void> {
  const clearHistory = useClearContactHistory();
  return useRouteMutation({ mutationFn: emptyTrash, onSettled: clearHistory });
}
