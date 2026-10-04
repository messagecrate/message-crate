import { describe, expect, it } from "vitest";
import { buildAssetPath } from "./assetUrl.ts";

describe("buildAssetPath", () => {
  it("names the asset by its sha alone", () => {
    expect(buildAssetPath("abc123")).toBe("/v1/assets/abc123");
  });

  it("encodes special characters", () => {
    expect(buildAssetPath("dead beef")).toBe("/v1/assets/dead%20beef");
  });

  it("rejects an empty sha", () => {
    expect(() => buildAssetPath("")).toThrow();
    expect(() => buildAssetPath("  ")).toThrow();
  });

  it("names the preview and the thumbnail under the original's sha", () => {
    expect(buildAssetPath("abc123", "preview")).toBe("/v1/assets/abc123/preview");
    expect(buildAssetPath("abc123", "thumbnail")).toBe("/v1/assets/abc123/thumbnail");
    expect(buildAssetPath("abc123", "original")).toBe("/v1/assets/abc123");
  });
});
