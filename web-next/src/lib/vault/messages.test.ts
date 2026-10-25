import assert from "node:assert/strict";
import { describe, it } from "node:test";

import type { Schemas } from "./client";
import { toAttachment } from "./messages";

const SHA = "c".repeat(64);

function attachment(over: Partial<Schemas["Attachment"]> = {}): Schemas["Attachment"] {
  return {
    is_sticker: false,
    shown_as_is: false,
    mime_type: "audio/wav",
    missing_reason: null,
    original_name: "note.wav",
    path: null,
    preview_mime_type: null,
    preview_not_made_reason: null,
    sha256: SHA,
    thumbnail_mime_type: null,
    thumbnail_not_made_reason: null,
    transcription: null,
    ...over,
  };
}

describe("toAttachment", () => {
  it("offers the Preview and Thumbnail the server holds", () => {
    const row = toAttachment(
      attachment({
        preview_mime_type: "audio/mpeg",
        thumbnail_mime_type: "image/jpeg",
      }),
      0,
    );
    assert.equal(row.previewMimeType, "audio/mpeg");
    assert.equal(row.previewAssetsPath, SHA);
    assert.equal(row.thumbnailMimeType, "image/jpeg");
    assert.equal(row.thumbnailAssetsPath, SHA);
    assert.equal(row.shownAsIs, false);
  });

  it("offers neither when the server holds neither", () => {
    const row = toAttachment(attachment({ shown_as_is: true }), 0);
    assert.equal(row.previewAssetsPath, null);
    assert.equal(row.thumbnailAssetsPath, null);
    assert.equal(row.assetsPath, SHA);
    assert.equal(row.shownAsIs, true);
    assert.equal(row.thumbnailNotMadeReason, null);
  });

  it("carries why the server could not make the Thumbnail", () => {
    const row = toAttachment(
      attachment({ shown_as_is: true, thumbnail_not_made_reason: "Invalid data" }),
      0,
    );
    assert.equal(row.thumbnailAssetsPath, null);
    assert.equal(row.thumbnailNotMadeReason, "Invalid data");
  });
});
