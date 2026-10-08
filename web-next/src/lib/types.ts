import type { IdentityType } from "./handleKind";

export type ContactSection =
  | "all"
  | "no-messages"
  | "no-label"
  | { label: string };

/** One handle attached to a contact. */
export type ContactHandle = {
  raw: string;
  handle_type: IdentityType;
  service: string | null;
  /** Review note when the handle's normalized form is ambiguous (not E.164). */
  normalizedNote: string | null;
};

export type ContactListItem = {
  id: number;
  displayName: string;
  /** Stored display name. */
  preferredName: string | null;
  preferredHandle: string | null;
  /** Type of {@link preferredHandle} (null when the contact has no handles). */
  handleType: IdentityType | null;
  /** Derived from preferredName (first space split) for search/API compat. */
  firstName: string | null;
  /** Derived from preferredName (first space split) for search/API compat. */
  lastName: string | null;
  sortFirst: string;
  sortLast: string;
  letter: string;
  labels: string[];
  /** Soft-deduped 1:1 message total (Combined view). */
  messageCount: number;
  /** Distinct group chats this contact participates in (non-trashed). */
  groupMessageCount: number;
  /** Earliest 1:1 message date (YYYY-MM-DD), if any. */
  dateStart: string | null;
  /** Latest 1:1 message date (YYYY-MM-DD), if any. */
  dateEnd: string | null;
  /** When the contact’s address-book shape last changed (SQLite datetime). */
  lastModified: string;
};

export type ContactDetail = ContactListItem & {
  /** Every handle on the contact with its type (phones first). */
  handles: ContactHandle[];
  /** All handle raws (legacy name; equals handles.map(h => h.raw)). */
  phones: string[];
};

export type YearThread = {
  year: number;
  messageCount: number;
  /** Attachments on messages in this year thread. */
  attachmentCount: number;
  dateStart: string;
  dateEnd: string;
  conversationIds: number[];
};

export type GroupParticipant = {
  name: string;
  handle: string;
  handleType: IdentityType | null;
  contactId: number | null;
};

export type GroupChatThread = {
  conversationId: number;
  conversationIds: number[];
  title: string;
  titleFull: string;
  namedTitle: string | null;
  participantCount: number;
  participantNames: string[];
  participantHandles: string[];
  participants: GroupParticipant[];
  year: number;
  messageCount: number;
  dateStart: string;
  dateEnd: string;
};

/** One group conversation bucketed into a calendar year for the Groups page. */
export type GroupYearRow = {
  id: number;
  year: number;
  title: string;
  titleFull: string;
  namedTitle: string | null;
  participantCount: number;
  participantNames: string[];
  participantHandles: string[];
  participants: GroupParticipant[];
  /** Messages in this year only. */
  messageCount: number;
  dateStart: string;
  dateEnd: string;
  /** Full conversation range (all years). */
  conversationDateStart: string;
  conversationDateEnd: string;
  spansMultipleYears: boolean;
  /** Present on trashed group rows (`trashed_conversations.trashed_at`). */
  trashedAt?: string;
};

export type AttachmentRow = {
  id: number;
  mimeType: string | null;
  originalName: string | null;
  assetsPath: string | null;
  sha256: string | null;
  derivedMimeType: string | null;
  derivedAssetsPath: string | null;
  derivedSha256: string | null;
};

export type MessageRow = {
  id: number;
  /** Present when messages from multiple conversations are loaded together. */
  conversationId?: number;
  source: string;
  timestamp: string;
  isFromMe: boolean;
  sender: string | null;
  senderName: string;
  body: string | null;
  isAnnouncement: boolean;
  attachments: AttachmentRow[];
};

export type UnassignedHandle = {
  handle: string;
  /** Handle type from the handles table (null when unknown). */
  handleType?: IdentityType | null;
  displayName: string;
  nameAlias: string | null;
  messageCount: number;
  dateStart: string | null;
  dateEnd: string | null;
  sortKey: string;
  letter: string;
  /** Backup archive name — show "(Unverified)" when true. */
  unverified?: boolean;
  /** Trash-list metadata (Contacts trash three-pane). */
  trashKind?: "unassigned" | "messages_only" | "contact";
  contactId?: number;
  /** Name sort keys (trash / contacts). Falls back to handle when absent. */
  sortFirst?: string;
  sortLast?: string;
  firstName?: string | null;
  lastName?: string | null;
  /** Soft-trash timestamp when listed from Trash. */
  trashedAt?: string;
};

/** Soft-trashed contact (contact + 1:1 messages). */
export type TrashedContactItem = {
  kind: "contact";
  contactId: number;
  displayName: string;
  preferredHandle: string | null;
  handleCount: number;
  messageCount: number;
  sortKey: string;
  letter: string;
  sortFirst: string;
  sortLast: string;
  firstName: string | null;
  lastName: string | null;
  trashedAt: string;
};

/** Soft-trashed 1:1 handle still linked to a live contact. */
export type TrashedContactMessagesItem = {
  kind: "messages_only";
  contactId: number;
  handle: string;
  displayName: string;
  messageCount: number;
  sortKey: string;
  letter: string;
  sortFirst: string;
  sortLast: string;
  firstName: string | null;
  lastName: string | null;
  trashedAt: string;
};

export type HomeStats = {
  /** Every non-trashed contact. */
  all: number;
  noMessages: number;
  groupChats: number;
  /** Soft-deduped messages (`duplicate_of IS NULL`). */
  messages: number;
  /** Cross-source copies marked as duplicates. */
  messageDuplicates: number;
  /** Total contact rows in the DB. */
  contacts: number;
  sentMessages: number;
  receivedMessages: number;
  attachments: number;
  sources: number;
  dateStart: string | null;
  dateEnd: string | null;
  recentContacts: Array<{
    id: number;
    displayName: string;
    messageCount: number;
    groupChatCount: number;
    dateEnd: string;
  }>;
};
