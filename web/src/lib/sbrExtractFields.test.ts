import { describe, expect, it } from "vitest";
import { sbrExtractFields } from "./sbrExtractFields";

describe("sbrExtractFields", () => {
  it("includes attachment media and owner phones for SMS Backup & Restore", () => {
    expect(
      sbrExtractFields({
        attachmentMedia: "convert",
        maxResolution: "1080p",
        maxFps: "30",
        minSizeMb: "20",
        ownerPhones: ["+15551111", "+15552222"],
        obfuscate: true,
      }),
    ).toEqual({
      attachment_media: "convert",
      media_max_resolution: "1080p",
      media_max_fps: "30",
      media_min_size: "20",
      owner_phones: ["+15551111", "+15552222"],
      obfuscate: true,
    });
  });
});
