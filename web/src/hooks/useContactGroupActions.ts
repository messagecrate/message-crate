import { contactGroups } from "../lib/contactGroups";
import { useNameCollectionActions } from "./useNameCollectionActions";

/** Create, rename, delete, and set membership on Contact Groups. */
export function useContactGroupActions() {
  return useNameCollectionActions(contactGroups);
}
