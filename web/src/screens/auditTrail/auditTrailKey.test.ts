import { describe, expect, it } from "vitest";
import { type AuditTrailOf, auditTrailKey, auditTrailOf } from "./useAuditTrail";

describe("auditTrailKey", () => {
  it("names each trail apart and reads it back, a live and a deleted account of one number included", () => {
    const trails: AuditTrailOf[] = [
      { kind: "all" },
      { kind: "own" },
      { kind: "account", id: 7 },
      { kind: "deleted", id: 7 },
    ];
    const keys = trails.map(auditTrailKey);
    expect(new Set(keys).size).toBe(trails.length);
    expect(keys.map(auditTrailOf)).toEqual(trails);
  });

  it("reads a key it did not make as every account's trail", () => {
    expect(auditTrailOf("deleted:")).toEqual({ kind: "all" });
    expect(auditTrailOf("someone")).toEqual({ kind: "all" });
  });
});
