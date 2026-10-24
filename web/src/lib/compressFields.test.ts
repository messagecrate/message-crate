import { describe, expect, it } from "vitest";
import { DEFAULT_ATTACHMENT_CHOICES } from "./attachmentChoices";
import { compressFieldErrors, MAX_FPS_INVALID, MIN_VIDEO_SIZE_INVALID } from "./compressFields";

const compress = (over: { maxFps?: string; minSizeMb?: string }) =>
  compressFieldErrors({ ...DEFAULT_ATTACHMENT_CHOICES, attachmentMedia: "compress", ...over });

// The desktop app's own checks (crates/libs/media/src/lib.rs) are tested on
// the same values, so the two copies of the rules stay in step (#3167).
describe("compressFieldErrors", () => {
  it.each([
    ["", undefined],
    ["  ", undefined],
    ["0", MAX_FPS_INVALID],
    ["1", undefined],
    [" 24 ", undefined],
    ["240", undefined],
    ["241", MAX_FPS_INVALID],
    ["300", MAX_FPS_INVALID],
    ["20aaa", MAX_FPS_INVALID],
    ["29.97", MAX_FPS_INVALID],
    ["-5", MAX_FPS_INVALID],
    ["+5", MAX_FPS_INVALID],
    ["1048576", MAX_FPS_INVALID],
  ])("reads Max FPS %j as %s", (maxFps, error) => {
    expect(compress({ maxFps }).maxFps).toBe(error);
  });

  it.each([
    ["", undefined],
    ["0", MIN_VIDEO_SIZE_INVALID],
    ["1", undefined],
    ["240", undefined],
    ["241", undefined],
    ["1048576", undefined],
    ["1048577", MIN_VIDEO_SIZE_INVALID],
    ["20aaa", MIN_VIDEO_SIZE_INVALID],
    ["29.97", MIN_VIDEO_SIZE_INVALID],
    ["20MB", MIN_VIDEO_SIZE_INVALID],
    ["-1", MIN_VIDEO_SIZE_INVALID],
  ])("reads Minimum Video File Size %j as %s", (minSizeMb, error) => {
    expect(compress({ minSizeMb }).minSizeMb).toBe(error);
  });

  it("checks nothing unless Convert & Compress media is picked", () => {
    for (const attachmentMedia of ["copy", "convert", "skip"] as const) {
      expect(
        compressFieldErrors({
          ...DEFAULT_ATTACHMENT_CHOICES,
          attachmentMedia,
          maxFps: "20aaa",
          minSizeMb: "0",
        }),
      ).toEqual({});
    }
  });
});
