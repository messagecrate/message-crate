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
