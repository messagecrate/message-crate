import { useSetNamedSetMembers } from "../../hooks/useNameCollectionActions";
import { contactGroups } from "../../lib/contactGroups";

/** Put contacts in or out of one Contact Group, drawn before the server answers. */
export function useSetContactGroupMembers() {
  return useSetNamedSetMembers(contactGroups);
}
