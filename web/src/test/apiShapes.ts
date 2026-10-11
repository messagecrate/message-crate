/**
 * Complete answers as the server sends them, for test fixtures: every field is
 * present, and `null` where it has no value. Pass the fields a test cares about.
 */

import type { ActiveImportRun } from "../lib/importRun";
import type { components } from "../lib/serverApi.types";

type Schema = components["schemas"];

/**
 * An attachment with every field `null`, and `is_sticker` and `shown_as_is`
 * false, as the server answers one it has not yet looked at.
 */
export function attachment(fields: Partial<Schema["Attachment"]> = {}): Schema["Attachment"] {
  return {
    is_sticker: false,
    mime_type: null,
    missing_reason: null,
    original_name: null,
    path: null,
    preview_mime_type: null,
    sha256: null,
    shown_as_is: false,
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
    time_precision: "milliseconds",
    sort_order: 0,
    is_from_me: false,
    is_announcement: false,
    reply_count: 0,
    sender: null,
    subject: null,
    text: null,
    deletion: null,
    owner: null,
    reply_to: null,
    attachments: [],
    tapbacks: [],
    earlier_versions: [],
    matched_earlier_version: false,
    backup_taken_at: null,
    conversation: {
      id: 1,
      chat_identifier: "x",
      conversation_type: "individual",
      is_group: false,
      group_title: null,
      shown_title: null,
      participants: [],
    },
    ...fields,
  };
}

/**
 * A received iMessage reading "hi" from +1555, in a one-to-one conversation
 * with Ada at +1555.
 */
export function imessageMessage(fields: Partial<Schema["Message"]> = {}): Schema["Message"] {
  return message({
    service: "iMessage",
    sender: "+1555",
    text: "hi",
    conversation: {
      id: 1,
      chat_identifier: "x",
      conversation_type: "individual",
      is_group: false,
      group_title: null,
      shown_title: null,
      participants: [participant({ identity: "+1555", name: "Ada" })],
    },
    ...fields,
  });
}

/**
 * An Apple Messages Import Run from an iPhone backup on this device, running,
 * at the Upload Stage, with no fingerprint, identities, or summary. Unlike the
 * other builders here, it is the answer as `getActiveImportRun` in lib/importRun.ts
 * hands it on, since that is the shape the Import screens read.
 */
export function activeImportRun(fields: Partial<ActiveImportRun> = {}): ActiveImportRun {
  return {
    id: 7,
    source: "imessage",
    mode: "append",
    status: "running",
    started_at: "2026-08-30T00:00:00Z",
    stage: "upload",
    run_dir: "/home/u/message-crate/staging-260830",
    device_id: "this-device",
    form: { source: "imessage-ios" },
    source_fingerprint: null,
    source_identities: null,
    summary: null,
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

/**
 * An active account that is not the owner or the Demo Account, with a
 * password, no messages, and every optional field `null` or empty.
 */
export function account(
  fields: Partial<Schema["Account"]> & Pick<Schema["Account"], "account_id" | "username">,
): Schema["Account"] {
  return {
    preferred_name: null,
    app: null,
    app_build: null,
    can_delete: false,
    can_export: true,
    can_import: true,
    disabled: false,
    emails: [],
    has_password: true,
    is_demo: false,
    is_owner: false,
    last_login_at: null,
    message_count: 0,
    must_set_up_profile: false,
    phones: [],
    storage_bytes: 0,
    time_zone: "UTC",
    ...fields,
  };
}
