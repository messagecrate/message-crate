import {
  useNameCollectionActions,
  useSetNamedSetMembers,
} from "../../hooks/useNameCollectionActions";
import { messageTags } from "../../lib/messageTags";

/** Create, rename, delete, and set membership on Message Tags. */
export function useMessageTagActions() {
  return useNameCollectionActions(messageTags);
}

/** Put conversations in or out of one Message Tag, drawn before the server answers. */
export function useSetMessageTagMembers() {
  return useSetNamedSetMembers(messageTags);
}
