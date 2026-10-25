import assert from "node:assert/strict";
import { describe, it } from "node:test";

import {
  attachmentMedia,
  noPreviewNote,
  noThumbnailNote,
  threadDrawing,
} from "./attachmentMedia";
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
    thumbnailNotMadeReason: null,
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

  it("opens a HEIC in no version with the original variant", () => {
    const media = attachmentMedia(
      "sms",
      row({ mimeType: "image/heic", shownAsIs: false }),
      "original",
    );
    assert.equal(media.full, "none");
    assert.equal(media.url, `/api/assets/sms/${SHA}`);
  });

  it("says why no Preview could be made, in web's words", () => {
    assert.equal(
      noPreviewNote(row({ previewNotMadeReason: "Invalid data" })),
      "No Preview could be made: Invalid data",
    );
    assert.equal(noPreviewNote(row()), null);
  });

  it("uses only the original with the original variant", () => {
    const media = attachmentMedia(
      "sms",
      row({
        previewMimeType: "image/jpeg",
        previewAssetsPath: SHA,
        thumbnailMimeType: "image/jpeg",
        thumbnailAssetsPath: SHA,
        shownAsIs: true,
      }),
      "original",
    );
    assert.equal(media.full, "original");
    assert.equal(media.url, `/api/assets/sms/${SHA}`);
    assert.equal(media.thumbnailUrl, null);
  });
});

describe("threadDrawing", () => {
  function drawing(over: Partial<AttachmentRow>, variant: "converted" | "original" = "converted") {
    return threadDrawing(attachmentMedia("imessage", row(over), variant), variant);
  }

  it("draws a picture's Thumbnail once the server has made one", () => {
    assert.equal(
      drawing({ thumbnailMimeType: "image/jpeg", thumbnailAssetsPath: SHA }),
      "thumbnail",
    );
  });

  it("draws a placeholder, never the original, for a picture with no Thumbnail yet", () => {
    assert.equal(drawing({}), "placeholder");
  });

  it("draws a placeholder for a picture whose Thumbnail could not be made", () => {
    assert.equal(drawing({ thumbnailNotMadeReason: "Invalid data" }), "placeholder");
  });

  it("draws the original picture with the original variant", () => {
    assert.equal(drawing({}, "original"), "original");
  });

  it("links a file that is not a picture", () => {
    assert.equal(drawing({ mimeType: "application/pdf", originalName: "a.pdf" }), "link");
  });

  it("says why when the attachment opens in no version", () => {
    assert.equal(drawing({ mimeType: "image/heic", shownAsIs: false }), "notice");
  });

  it("says the attachment is missing when the server holds no bytes", () => {
    assert.equal(drawing({ assetsPath: null, sha256: null }), "missing");
  });
});

describe("noThumbnailNote", () => {
  it("says why no Thumbnail could be made, in web's words", () => {
    assert.equal(
      noThumbnailNote(row({ thumbnailNotMadeReason: "Invalid data" })),
      "No Thumbnail could be made: Invalid data",
    );
    assert.equal(noThumbnailNote(row()), null);
  });
});
