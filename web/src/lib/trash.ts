import type { UseMutationResult } from "@tanstack/react-query";
import { useRouteMutation } from "./routeQuery";
import {
  deleteContact,
  deleteConversation,
  emptyTrash,
  restoreContact,
  restoreConversation,
  trashContact,
  trashConversation,
} from "./serverApi";

/**
 * Trash is a soft marker on two different nouns — conversations and contacts
 * — set and cleared by four idempotent routes, and the one door to permanent
 * deletion: Delete on a trashed row, and Empty Trash for everything in it.
 *
 * Unlike Contact Groups and Message Tags this is not a `nameCollection`:
 * there is no name, no membership, and nothing to look an id up by — the
 * caller already has the conversation or contact id. Each one is a plain
 * mutation over one server route.
 */

export function useTrashConversation(): UseMutationResult<void, Error, number> {
  return useRouteMutation({ mutationFn: trashConversation });
}

export function useRestoreConversation(): UseMutationResult<void, Error, number> {
  return useRouteMutation({ mutationFn: restoreConversation });
}

/** Permanently delete a trashed conversation. */
export function useDeleteConversation(): UseMutationResult<void, Error, number> {
  return useRouteMutation({ mutationFn: deleteConversation });
}

export function useTrashContact(): UseMutationResult<void, Error, string | number> {
  return useRouteMutation({ mutationFn: trashContact });
}

export function useRestoreContact(): UseMutationResult<void, Error, string | number> {
  return useRouteMutation({ mutationFn: restoreContact });
}

/**
 * Delete a trashed contact: the name and details go and the contact becomes
 * Unknown again, its conversations untouched.
 */
export function useDeleteContact(): UseMutationResult<void, Error, string | number> {
  return useRouteMutation({ mutationFn: deleteContact });
}

/**
 * Empty the trash: what `useDeleteConversation` does to every trashed
 * conversation and `useDeleteContact` to every trashed contact, in one server
 * call.
 */
export function useEmptyTrash(): UseMutationResult<void, Error, void> {
  return useRouteMutation({ mutationFn: emptyTrash });
}
