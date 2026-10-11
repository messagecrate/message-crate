import type { ContactBrowseTarget } from "../components/contactDrawer/contactDrawerTypes";
import { forHandle, forPerson, withKind } from "./searchQuery";

/** Search query used when browsing a contact's conversations from the drawer. */
export function contactBrowseQuery({ contactId, kind, handle }: ContactBrowseTarget): string {
  const h = handle?.trim();
  return withKind(h ? forHandle(h) : forPerson("with", contactId), kind);
}
