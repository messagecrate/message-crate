import { describe, expect, it } from "vitest";
import { MIB } from "../test/units";
import { formatBytes } from "./formatBytes";

describe("formatBytes", () => {
  it("handles zero and non-finite", () => {
    expect(formatBytes(0)).toBe("0 B");
    expect(formatBytes(-1)).toBe("0 B");
    expect(formatBytes(Number.NaN)).toBe("0 B");
  });

  it("scales units", () => {
    expect(formatBytes(512)).toBe("512 B");
    expect(formatBytes(1536)).toBe("1.5 KB");
    expect(formatBytes(5 * MIB)).toBe("5.0 MB");
  });
});
