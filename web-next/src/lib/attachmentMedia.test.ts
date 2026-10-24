import assert from "node:assert/strict";
import { describe, it } from "node:test";

import { attachmentMedia } from "./attachmentMedia";
import type { AttachmentRow } from "./types";

const SHA = "a".repeat(64);

function row(over: Partial<AttachmentRow> = {}): AttachmentRow {
  return {
    id: 0,
    mimeType: "image/jpeg",
    originalName: "photo.jpg",
    assetsPath: SHA,
    sha256: SHA,
    previewMimeType: null,
    previewAssetsPath: null,
    previewSha256: null,
    previewNotMadeReason: null,
    thumbnailMimeType: null,
    thumbnailAssetsPath: null,
    shownAsIs: true,
    ...over,
  };
}

describe("attachmentMedia", () => {
  it("opens the original a browser shows as it is, and draws its Thumbnail", () => {
    const media = attachmentMedia(
      "imessage",
      row({ thumbnailMimeType: "image/jpeg", thumbnailAssetsPath: SHA }),
      "converted",
    );
    assert.equal(media.full, "original");
    assert.equal(media.url, `/api/assets/imessage/${SHA}`);
    assert.equal(media.mimeType, "image/jpeg");
    assert.equal(media.thumbnailUrl, `/api/assets_thumbnail/imessage/${SHA}`);
  });

  it("opens the Preview of an original a browser cannot show", () => {
    const media = attachmentMedia(
      "sms",
      row({
        mimeType: "audio/wav",
        previewMimeType: "audio/mpeg",
        previewAssetsPath: SHA,
        shownAsIs: false,
      }),
      "converted",
    );
    assert.equal(media.full, "preview");
    assert.equal(media.url, `/api/assets_converted/sms/${SHA}`);
    assert.equal(media.mimeType, "audio/mpeg");
    assert.equal(media.thumbnailUrl, null);
  });

  it("opens neither when the server has made no Preview, though it has a Thumbnail", () => {
    const media = attachmentMedia(
      "sms",
      row({
        mimeType: "image/heic",
        shownAsIs: false,
        thumbnailMimeType: "image/jpeg",
        thumbnailAssetsPath: SHA,
      }),
      "converted",
    );
    assert.equal(media.full, "none");
    assert.equal(media.url, `/api/assets/sms/${SHA}`);
  });

  it("opens a file that is not a picture, video or sound as the original", () => {
    const media = attachmentMedia(
      "sms",
      row({ mimeType: "application/pdf", shownAsIs: false }),
      "converted",
    );
    assert.equal(media.full, "original");
    assert.equal(media.url, `/api/assets/sms/${SHA}`);
  });

  it("uses only the original with the original variant", () => {
    const media = attachmentMedia(
      "sms",
      row({
        previewMimeType: "image/jpeg",
        previewAssetsPath: SHA,
        thumbnailMimeType: "image/jpeg",
        thumbnailAssetsPath: SHA,
        shownAsIs: false,
      }),
      "original",
    );
    assert.equal(media.full, "original");
    assert.equal(media.url, `/api/assets/sms/${SHA}`);
    assert.equal(media.thumbnailUrl, null);
  });
});
