import { describe, expect, it } from "vitest";
import { mediaExtractFields, sbrExtractFields } from "./sbrExtractFields";

describe("sbrExtractFields", () => {
  it("includes attachment media and owner phones for SMS Backup & Restore", () => {
    expect(
      sbrExtractFields({
        attachmentMedia: "convert",
        maxResolution: "1080p",
        maxFps: "30",
        minSizeMb: "20",
        ownerPhones: ["+15550111", "+15550122"],
        obfuscate: true,
      }),
    ).toEqual({
      attachment_media: "convert",
      media_max_resolution: "1080p",
      media_max_fps: "30",
      media_min_size: "20",
      owner_phones: ["+15550111", "+15550122"],
      obfuscate: true,
    });
  });
});

describe("mediaExtractFields", () => {
  // #1469: the desktop app refuses an empty Minimum Video File Size by name,
  // as it does Max FPS, so a cleared field is sent as typed, not as 20.
  it("sends a cleared Minimum Video File Size as typed", () => {
    expect(
      mediaExtractFields({
        attachmentMedia: "compress",
        maxResolution: "720p",
        maxFps: "30",
        minSizeMb: "",
      }).media_min_size,
    ).toBe("");
  });
});
