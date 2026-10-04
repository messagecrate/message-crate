import { describe, expect, it } from "vitest";
import { imessageExtractFields } from "./imessageExtractFields";

describe("imessageExtractFields", () => {
  it("sends media and password for iPhone backup, not attachment root", () => {
    expect(
      imessageExtractFields({
        source: "imessage-ios",
        backupPassword: "pw",
        attachmentMedia: "convert",
        maxResolution: "1080p",
        maxFps: "30",
        minSizeMb: "20",
        obfuscate: true,
        attachmentRoot: "/should-not-send",
        appleContacts: "/should-not-send",
      }),
    ).toEqual({
      attachment_media: "convert",
      media_max_resolution: "1080p",
      media_max_fps: "30",
      media_min_size: "20",
      obfuscate: true,
      backup_password: "pw",
    });
  });

  it("omits empty extras on Mac and does not send a password", () => {
    expect(
      imessageExtractFields({
        source: "imessage-macos",
        backupPassword: "leftover",
        attachmentMedia: "copy",
        maxResolution: "720p",
        maxFps: "30",
        minSizeMb: "20",
        obfuscate: false,
        attachmentRoot: "  ",
        appleContacts: "",
      }),
    ).toEqual({
      attachment_media: "copy",
      media_max_resolution: "720p",
      media_max_fps: "30",
      media_min_size: "20",
    });
  });

  it("sends attachment root and contacts for jailbreak", () => {
    expect(
      imessageExtractFields({
        source: "imessage-jailbreak",
        backupPassword: "",
        attachmentMedia: "skip",
        maxResolution: "720p",
        maxFps: "30",
        minSizeMb: "20",
        obfuscate: false,
        attachmentRoot: "/mnt/iphone/Library/SMS",
        appleContacts: "/mnt/iphone/AddressBook.sqlitedb",
      }),
    ).toEqual({
      attachment_media: "skip",
      media_max_resolution: "720p",
      media_max_fps: "30",
      media_min_size: "20",
      attachment_root: "/mnt/iphone/Library/SMS",
      apple_contacts: "/mnt/iphone/AddressBook.sqlitedb",
    });
  });
});
