import { describe, expect, it } from "vitest";
import { attachment } from "../test/apiShapes";
import { missingAttachmentChipLabel } from "./missingAttachmentLabel.ts";

describe("missingAttachmentChipLabel", () => {
  it("formats too_large with name and mime", () => {
    expect(
      missingAttachmentChipLabel(
        attachment({
          original_name: "video.mp4",
          mime_type: "video/mp4",
          missing_reason: "too_large",
        }),
      ),
    ).toBe("video.mp4 · video/mp4 (missing — too large)");
  });

  it("formats file_missing from path basename when name missing", () => {
    expect(
      missingAttachmentChipLabel(
        attachment({
          path: "attachments/gone.bin",
          missing_reason: "file_missing",
        }),
      ),
    ).toBe("gone.bin (missing — file not found)");
  });

  it("says an attachment an export could not fetch was not fetched", () => {
    expect(
      missingAttachmentChipLabel(
        attachment({
          original_name: "menu.pdf",
          mime_type: "application/pdf",
          missing_reason: "not_fetched",
        }),
      ),
    ).toBe("menu.pdf · application/pdf (missing — not fetched by the export)");
  });

  it("labels a deliberately skipped attachment as skipped, not missing", () => {
    expect(
      missingAttachmentChipLabel(
        attachment({
          original_name: "IMG_0421.HEIC",
          mime_type: "image/heic",
          missing_reason: "not_copied",
        }),
      ),
    ).toBe("IMG_0421.HEIC · image/heic (skipped)");
  });

  it("keeps the ffmpeg detail from a convert_failed reason", () => {
    expect(
      missingAttachmentChipLabel(
        attachment({
          original_name: "clip.mov",
          missing_reason: "convert_failed: no video stream",
        }),
      ),
    ).toBe("clip.mov (could not be converted — no video stream)");
  });

  it("keeps an unrecognized reason visible", () => {
    expect(
      missingAttachmentChipLabel(
        attachment({ original_name: "a.bin", missing_reason: "weird_reason" }),
      ),
    ).toBe("a.bin (could not be imported — weird_reason)");
  });

  it("drops the trailing dash when a convert_failed reason has no detail", () => {
    expect(
      missingAttachmentChipLabel(
        attachment({ original_name: "clip.mov", missing_reason: "convert_failed: " }),
      ),
    ).toBe("clip.mov (could not be converted)");
  });

  it("treats no_path and null as plain missing", () => {
    expect(
      missingAttachmentChipLabel(attachment({ original_name: "a.bin", missing_reason: "no_path" })),
    ).toBe("a.bin (missing)");
    expect(missingAttachmentChipLabel(attachment({ original_name: "a.bin" }))).toBe(
      "a.bin (missing)",
    );
  });
});
