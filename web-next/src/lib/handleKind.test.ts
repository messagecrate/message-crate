import assert from "node:assert/strict";
import { describe, it } from "node:test";

import {
  inferHandleType,
  normalizeHandle,
  phoneReviewNote,
} from "./handleKind";

describe("normalizeHandle guarded phone policy", () => {
  it("normalizes unambiguous US and +-prefixed values to E.164", () => {
    assert.equal(normalizeHandle("5555550100", "phone"), "+15555550100");
    assert.equal(normalizeHandle("15555550100", "phone"), "+15555550100");
    assert.equal(normalizeHandle("+15555550100", "phone"), "+15555550100");
    assert.equal(normalizeHandle("+44 20 7946 0750", "phone"), "+442079460750");
  });

  it("keeps ambiguous national numbers as digits (never +0…)", () => {
    // Trunk-zero national number: matches the import-time normalized form so
    // the review-flagged row is found, instead of fabricating +02079460000.
    assert.equal(normalizeHandle("020 7946 0000", "phone"), "02079460000");
    assert.equal(normalizeHandle("442079460000", "phone"), "442079460000");
    // A fabricated +0… value is never accepted as certain E.164.
    assert.equal(normalizeHandle("+02079460000", "phone"), "02079460000");
    // Short codes stay as-is.
    assert.equal(normalizeHandle("7535", "phone"), "7535");
  });

  it("computes review notes mirroring the server's reasons", () => {
    assert.equal(
      phoneReviewNote("020 7946 0000"),
      "USA needs 10 digits or 11 starting with 1",
    );
    assert.equal(
      phoneReviewNote("+02079460000"),
      "international country code cannot start with 0",
    );
    assert.equal(phoneReviewNote("+15555550100"), null);
    assert.equal(phoneReviewNote(""), null);
    assert.equal(phoneReviewNote("person@example.com"), null);
  });

  it("infers handle types from shape", () => {
    assert.equal(inferHandleType("020 7946 0000"), "phone");
    assert.equal(inferHandleType("+442079460750"), "phone");
    assert.equal(inferHandleType("person@example.com"), "email");
    assert.equal(inferHandleType("discord#1234"), "other");
  });
});
