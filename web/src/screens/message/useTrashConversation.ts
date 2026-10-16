import type { UseMutationResult } from "@tanstack/react-query";
import { useRouteMutation } from "../../hooks/useRouteQuery";
import { trashConversation } from "../../lib/serverApi";

/** Move a conversation to the Trash. The Trash screen restores it or deletes it for good. */
export function useTrashConversation(): UseMutationResult<void, Error, number> {
  return useRouteMutation({ mutationFn: trashConversation });
}
