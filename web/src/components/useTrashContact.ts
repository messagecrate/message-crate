import type { UseMutationResult } from "@tanstack/react-query";
import { useRouteMutation } from "../hooks/useRouteQuery";
import { trashContact } from "../lib/serverApi";

/** Move a contact to the Trash. The Trash screen restores it or deletes it for good. */
export function useTrashContact(): UseMutationResult<void, Error, string | number> {
  return useRouteMutation({ mutationFn: trashContact });
}
