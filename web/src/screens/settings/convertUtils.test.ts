import { describe, expect, it } from "vitest";
import { sameDirectory } from "./convertUtils";

describe("sameDirectory", () => {
  it("treats identical paths as the same directory", () => {
    expect(sameDirectory("/home/demo/out", "/home/demo/out")).toBe(true);
  });

  it("ignores surrounding whitespace and trailing slashes", () => {
    expect(sameDirectory(" /home/demo/out/ ", "/home/demo/out")).toBe(true);
    expect(sameDirectory("C:\\exports\\", "C:\\exports")).toBe(true);
  });

  it("does not call two empty fields the same directory", () => {
    // Both fields start empty. The button is disabled for emptiness, not for
    // a directory clash, so no clash message should show yet.
    expect(sameDirectory("", "")).toBe(false);
    expect(sameDirectory("  ", "")).toBe(false);
  });

  it("keeps a bare root distinct from an empty field", () => {
    expect(sameDirectory("/", "")).toBe(false);
    expect(sameDirectory("/", "/")).toBe(true);
  });

  it("treats different paths as different directories", () => {
    expect(sameDirectory("/home/demo/out", "/home/demo/out2")).toBe(false);
    expect(sameDirectory("/home/demo/in", "/home/demo/in/sub")).toBe(false);
  });
});
