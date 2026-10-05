/**
 * Complete answers as the server sends them, for test fixtures: every field is
 * present, and `null` where it has no value. Pass the fields a test cares about.
 */

import type { components } from "../lib/serverApi.types";

type Schema = components["schemas"];

/** An attachment with every field `null` and `is_sticker` false. */
export function attachment(fields: Partial<Schema["Attachment"]> = {}): Schema["Attachment"] {
  return {
    is_sticker: false,
    mime_type: null,
    missing_reason: null,
    original_name: null,
    path: null,
    preview_mime_type: null,
    sha256: null,
    thumbnail_mime_type: null,
    transcription: null,
    ...fields,
  };
}

/** A participant named `name`, with no service, contact, or identity. */
export function participant(
  fields: Partial<Schema["Participant"]> & Pick<Schema["Participant"], "name">,
): Schema["Participant"] {
  return { contact_id: null, identity: null, service: null, ...fields };
}

/**
 * A received message with no text, in a one-to-one conversation with nobody
 * in it: every nullable field `null`, every list empty, every flag false.
 */
export function message(fields: Partial<Schema["Message"]> = {}): Schema["Message"] {
  return {
    id: 1,
    source: "imessage",
    service: null,
    guid: "g1",
    timestamp: "2026-08-11T15:04:00Z",
    sort_order: 0,
    is_from_me: false,
    is_announcement: false,
    is_reply: false,
    num_replies: 0,
    sender: null,
    subject: null,
    text: null,
    deletion: null,
    owner: null,
    thread_originator_guid: null,
    thread_originator_part: null,
    attachments: [],
    tapbacks: [],
    earlier_versions: [],
    matched_earlier_version: false,
    conversation: {
      id: 1,
      chat_identifier: "x",
      conversation_type: "individual",
      is_group: false,
      group_title: null,
      label: null,
      participants: [],
    },
    ...fields,
  };
}

/** An Audit Trail entry by the account holder, with every count and name `null`. */
export function auditEntry(
  fields: Partial<Schema["AuditEntry"]> & Pick<Schema["AuditEntry"], "action">,
): Schema["AuditEntry"] {
  return {
    id: 1,
    at: "2026-10-02T10:00:00+00:00",
    actor: "holder",
    account_id: null,
    username: null,
    api_token_hint: null,
    api_token_label: null,
    app: null,
    app_build: null,
    attachments: null,
    bytes: null,
    contacts: null,
    contacts_created: null,
    contacts_deleted: null,
    contacts_updated: null,
    conversations: null,
    credential: null,
    identities: null,
    messages: null,
    mode: null,
    permissions_added: null,
    permissions_removed: null,
    reason: null,
    scope_kind: null,
    scope_list: null,
    source: null,
    status: null,
    ...fields,
  };
}
