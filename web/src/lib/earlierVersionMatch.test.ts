import { describe, expect, it } from "vitest";
import { sampleMessage } from "../test/messages";
import { matchedVersionIndexes, withMatchedVersions } from "./earlierVersionMatch";

const VERSIONS = [
  { part_index: 0, text: "See you at noon", edited_at: null, matched: false },
  { part_index: 0, text: "See you at half past noon", edited_at: null, matched: false },
  { part_index: 0, text: "See you at two", edited_at: null, matched: false },
];

describe("matchedVersionIndexes", () => {
  it("is where the versions a search found the message by sit in its list", () => {
    const hit = sampleMessage({
      matched_earlier_version: true,
      earlier_versions: VERSIONS.map((v, i) => ({ ...v, matched: i !== 1 })),
    });
    expect(matchedVersionIndexes(hit)).toEqual([0, 2]);
  });

  it("is empty for a hit its final text matched", () => {
    expect(matchedVersionIndexes(sampleMessage({ earlier_versions: VERSIONS }))).toEqual([]);
  });
});

describe("withMatchedVersions", () => {
  it("marks the message found only by the versions named, and those versions alone", () => {
    const shown = withMatchedVersions(sampleMessage({ earlier_versions: VERSIONS }), [1]);
    expect(shown.matched_earlier_version).toBe(true);
    expect(shown.earlier_versions.map((v) => v.matched)).toEqual([false, true, false]);
  });

  it("leaves the message as it is when no version it has is named", () => {
    const message = sampleMessage({ earlier_versions: VERSIONS });
    expect(withMatchedVersions(message, [])).toBe(message);
    expect(withMatchedVersions(message, [3])).toBe(message);
    const unedited = sampleMessage({});
    expect(withMatchedVersions(unedited, [0])).toBe(unedited);
  });
});
