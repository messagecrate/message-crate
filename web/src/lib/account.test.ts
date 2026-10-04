import { describe, expect, it } from "vitest";
import { fixedSettings } from "./account";

describe("fixedSettings", () => {
  it("fixes everything the server refuses to change on the Demo Account", () => {
    expect(fixedSettings({ is_demo: true })).toEqual({
      displayName: true,
      timeZone: true,
      identities: true,
      addressBook: true,
      statusAndPermissions: true,
      password: true,
      deleteMessages: true,
      deleteOwnAccount: true,
    });
  });

  it("fixes nothing on any other account", () => {
    expect(Object.values(fixedSettings({ is_demo: false })).some(Boolean)).toBe(false);
  });
});
