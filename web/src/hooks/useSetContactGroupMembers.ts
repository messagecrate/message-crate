import { contactGroups } from "../lib/contactGroups";
import { useSetNamedSetMembers } from "./useNameCollectionActions";

/** Put contacts in or out of one Contact Group, drawn before the server answers. */
export function useSetContactGroupMembers() {
  return useSetNamedSetMembers(contactGroups);
}
