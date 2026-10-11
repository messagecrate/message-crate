import { describe, expect, it } from "vitest";
import { attachment } from "../test/apiShapes";
import { attachmentKind, fullVersion, noPreviewNote } from "./attachmentMedia";

/**
 * The viewer's rule (`docs/architecture/media.md`, rule 2), as the server
 * answers it in `shown_as_is`: a wrong answer either opens bytes the browser
 * cannot draw or sends a JPEG to a Preview that will never exist.
 */
describe("fullVersion", () => {
  it("opens the original of a JPEG and of an H.264 MP4, which the server says are shown as they are", () => {
    const jpeg = attachment({ mime_type: "image/jpeg", sha256: "a", shown_as_is: true });
    const h264 = attachment({ mime_type: "video/mp4", sha256: "b", shown_as_is: true });

    expect(fullVersion(jpeg)).toBe("original");
    expect(fullVersion(h264)).toBe("original");
  });

  it("opens nothing of a HEVC MP4 without a Preview, though its type is MP4", () => {
    // Only the file tells an MP4's codec, so the web app reads the server's
    // answer instead of guessing from the type.
    expect(
      fullVersion(attachment({ mime_type: "video/mp4", sha256: "a", shown_as_is: false })),
    ).toBe("none");
  });

  it("opens the Preview of an original that is not shown as it is, once the server made one", () => {
    expect(
      fullVersion(attachment({ mime_type: "image/heic", preview_mime_type: "image/jpeg" })),
    ).toBe("preview");
    expect(
      fullVersion(attachment({ mime_type: "video/mp4", preview_mime_type: "video/mp4" })),
    ).toBe("preview");
  });

  it("opens nothing while the server has not said the original is shown as it is and no Preview exists", () => {
    expect(fullVersion(attachment({ mime_type: "image/heic" }))).toBe("none");
    expect(fullVersion(attachment({ mime_type: "image/jpeg" }))).toBe("none");
    expect(fullVersion(attachment({}))).toBe("none");
  });
});

/**
 * Why there is no Preview is said only where nothing opens: a reason beside a
 * version that opens would tell the reader of a failure they never meet.
 */
describe("noPreviewNote", () => {
  const reason = "Error opening input file ab/a.heic.";

  it("says why the server could not make the Preview of an attachment that opens in nothing", () => {
    expect(
      noPreviewNote(attachment({ mime_type: "image/heic", preview_not_made_reason: reason })),
    ).toBe(`No Preview could be made: ${reason}`);
  });

  it("says nothing while no try has failed, or when a version opens", () => {
    expect(noPreviewNote(attachment({ mime_type: "image/heic" }))).toBeNull();
    expect(
      noPreviewNote(attachment({ shown_as_is: true, preview_not_made_reason: reason })),
    ).toBeNull();
    expect(
      noPreviewNote(
        attachment({ preview_mime_type: "image/jpeg", preview_not_made_reason: reason }),
      ),
    ).toBeNull();
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

  it("reads the declared type first, as the server does for a stored original", () => {
    // A stored original has no extension, so the server types it by what
    // the import declared, octet-stream included.
    expect(
      attachmentKind(
        attachment({ original_name: "IMG_1.jpg", mime_type: "application/octet-stream" }),
      ),
    ).toBe("file");
    expect(attachmentKind(attachment({ mime_type: "Image/JPG; q=1" }))).toBe("image");
  });
});
