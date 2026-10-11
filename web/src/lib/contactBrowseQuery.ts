import { type ConversationKind, forHandle, forPerson, withKind } from "./searchQuery";

/** Which of a contact's conversations to open: a kind, and at most one of its identities. */
export interface ContactBrowseScope {
  kind: ConversationKind;
  handle?: string;
}

/** The contact, or the one identity of it, whose conversations to open. */
export interface ContactBrowseTarget extends ContactBrowseScope {
  contactId: string;
}

/** Search query used when browsing a contact's conversations from the drawer. */
export function contactBrowseQuery({ contactId, kind, handle }: ContactBrowseTarget): string {
  const h = handle?.trim();
  return withKind(h ? forHandle(h) : forPerson("with", contactId), kind);
}
