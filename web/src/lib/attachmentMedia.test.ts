import { describe, expect, it } from "vitest";
import { attachment } from "../test/apiShapes";
import { attachmentKind, fullVersion } from "./attachmentMedia";

/**
 * The viewer's rule (`docs/architecture/media.md`, rule 2), on the type
 * alone: a wrong answer either opens bytes the browser cannot draw or sends a
 * JPEG to a Preview that will never exist.
 */
describe("fullVersion", () => {
  it("opens the original of each type every browser shows", () => {
    for (const mime_type of [
      "image/jpeg",
      "image/png",
      "image/gif",
      "image/webp",
      "audio/mpeg",
      "video/mp4",
    ]) {
      expect(fullVersion(attachment({ mime_type, sha256: "a" })), mime_type).toBe("original");
    }
  });

  it("reads another spelling of a type, and parameters, as the type", () => {
    expect(fullVersion(attachment({ mime_type: "image/jpg", sha256: "a" }))).toBe("original");
    expect(fullVersion(attachment({ mime_type: "Audio/MP3; codecs=mp3", sha256: "a" }))).toBe(
      "original",
    );
  });

  it("reads the declared type first, as the server does for a stored original", () => {
    // A stored original has no extension, so the server types it by what
    // the import declared, octet-stream included, and makes nothing of it.
    expect(
      fullVersion(attachment({ path: "a/IMG_1.jpg", mime_type: "image/heic", sha256: "a" })),
    ).toBe("none");
    expect(
      attachmentKind(
        attachment({ original_name: "IMG_1.jpg", mime_type: "application/octet-stream" }),
      ),
    ).toBe("file");
  });

  it("reads the type from the file name when the import named none", () => {
    expect(fullVersion(attachment({ original_name: "IMG_1.JPG", sha256: "a" }))).toBe("original");
    expect(fullVersion(attachment({ original_name: "IMG_1.HEIC", sha256: "a" }))).toBe("none");
  });

  it("opens the Preview whenever the server made one, a HEVC MP4 included", () => {
    expect(
      fullVersion(attachment({ mime_type: "image/heic", preview_mime_type: "image/jpeg" })),
    ).toBe("preview");
    expect(
      fullVersion(attachment({ mime_type: "video/mp4", preview_mime_type: "video/mp4" })),
    ).toBe("preview");
  });

  it("opens nothing of a type browsers often cannot show while it has no Preview", () => {
    expect(fullVersion(attachment({ mime_type: "image/heic" }))).toBe("none");
    expect(fullVersion(attachment({ mime_type: "video/quicktime" }))).toBe("none");
    expect(fullVersion(attachment({ mime_type: "audio/amr" }))).toBe("none");
    expect(fullVersion(attachment({}))).toBe("none");
  });
});

describe("attachmentKind", () => {
  it("sorts by type, then by file name, then by the Preview's type", () => {
    expect(attachmentKind(attachment({ mime_type: "image/heic" }))).toBe("image");
    expect(attachmentKind(attachment({ original_name: "clip.MOV" }))).toBe("video");
    expect(
      attachmentKind(attachment({ original_name: "voice", preview_mime_type: "audio/mpeg" })),
    ).toBe("audio");
    expect(attachmentKind(attachment({ mime_type: "application/pdf" }))).toBe("file");
  });
});
