import type { components } from "./serverApi.types";

type Schema = components["schemas"];

/*
 * Shapes the server returns come from the generated types, so a field renamed
 * on the server is a build error here rather than an empty screen. Shapes
 * below that the server never sends — desktop command arguments, progress
 * events, and the per-app extras on a message — stay hand-written.
 */

/** One participant in a conversation, as the conversation list returns them. */
export type Participant = Schema["Participant"];

/** One conversation in the browse list. */
export type Conversation = Schema["ConversationSummary"];

/** One participant on a message. */
export type MessageParticipant = Schema["Participant"];

/** The conversation a message belongs to, as a message carries it. */
export type MessageConversation = Schema["MessageConversation"];

/** One attachment on a message. */
export type MessageAttachment = Schema["Attachment"];

/** One tapback reaction on a message. */
export type MessageTapback = Schema["Tapback"];

/** One message, as `GET /v1/conversations/{id}/messages` returns it — the
 * same row shape the Export routes return, since one loader serves both. */
export type Message = Schema["Message"];

export type AttachmentMediaMode = "copy" | "convert" | "compress" | "skip";

export interface ExtractConfig {
  source: string;
  path: string;
  output_dir: string;
  backup_password?: string;
  attachment_media?: AttachmentMediaMode;
  media_max_resolution?: string;
  media_max_fps?: string;
  media_min_size?: string;
  obfuscate?: boolean;
  /** Zone iMazing dates are read in (an IANA name); iMazing carries none of its own. */
  timezone?: string;
  /** Owner phone numbers for the Android SMS sources and WhatsApp (repeatable). */
  owner_phones?: string[];
  /** Owner email addresses for SMS Backup+ (repeatable). */
  owner_emails?: string[];
  /** Alternate directory for Attachments and StickerCache (Mac and jailbreak). */
  attachment_root?: string;
  /** Path to an Apple AddressBook file (Mac and jailbreak). */
  apple_contacts?: string;
  /** WhatsApp decryption key (file path or crypt15 hex). Not an Apple backup password. */
  whatsapp_key?: string;
  /** WhatsApp contacts database (`wa.db` or ContactsV2.sqlite). */
  whatsapp_wa?: string;
  /** WhatsApp media directory override. */
  whatsapp_media?: string;
  /** Explicit WhatsApp message database (`msgstore.db`). */
  whatsapp_db?: string;
  /** iPhone WhatsApp Business default files (`--business`). */
  whatsapp_business?: boolean;
  /** Continue an interrupted export in the same directory: previous output is
   * kept and conversations already written are skipped. */
  resume?: boolean;
  /** The server's attachment size limit, in bytes, as stored with the Import
   * Run. Staging records it in the directory with the run's media settings. */
  asset_max_bytes: number;
}

export interface ExtractErrorEvent {
  detail: string;
  user_message?: string;
}

/**
 * One typed progress event from the desktop backend (`extract:progress`).
 * `setup` is a numbered step before any message is read (decrypting an
 * iPhone backup, caching chat tables); its label arrives as `status` and
 * `done`/`total` are the step's position, not message counts.
 */
export interface ImportProgressEvent {
  step: "setup" | "parse" | "attachments" | "prepare" | "check" | "media" | "upload";
  done: number;
  total: number;
  bytes_done?: number;
  bytes_total?: number;
  status?: string;
}

/**
 * One row for the Import Errors list, or the notes list (`extract:issue`),
 * sent the moment the stage records it.
 */
export interface ImportIssueEvent {
  /**
   * `skip` when the item was left out, `error` when it failed, `note` when
   * the stage did something with it worth knowing that is not a failure,
   * such as keeping a message with a caveat; a note joins the run's notes,
   * not its Import Errors. `resolved` is not a row: it says an earlier row
   * with the same stage and item no longer holds, as when Media converts a
   * file on a later try.
   */
  kind: "error" | "skip" | "note" | "resolved";
  step: "parse" | "attachments" | "prepare" | "media" | "upload";
  item: string;
  reason: string;
  /**
   * The conversation file an Upload row is about; it is the `item` too when
   * the row is about the whole conversation. A resumed Upload reads again
   * only the conversations not yet on the server, so this says which rows a
   * resume reports again. Absent on the other stages' rows.
   */
  conversation?: string;
}

/**
 * The Upload finished with one conversation file (`extract:file-done`):
 * `ok` sent it now, `skipped` found an earlier part of the run had sent it,
 * `failed` could not send it.
 */
export interface ImportFileDoneEvent {
  file: string;
  status: "ok" | "skipped" | "failed";
}

/**
 * What an Upload said of one conversation: an `ImportFileDoneEvent` status,
 * or `cancelled` from its report, for one a stop left unsent.
 */
export type ConversationStatus = ImportFileDoneEvent["status"] | "cancelled";
