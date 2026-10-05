import { describe, expect, it } from "vitest";
import {
  messagesSearch,
  openedAt,
  openedMatchedVersions,
  pickedMessageSort,
  resultsView,
} from "./resultsView";

const params = (s: string) => new URLSearchParams(s);

describe("resultsView", () => {
  it("is Messages only when the address says so", () => {
    expect(resultsView(params("view=messages"))).toBe("messages");
    expect(resultsView(params(""))).toBe("conversations");
    expect(resultsView(params("view=other"))).toBe("conversations");
  });
});

describe("pickedMessageSort", () => {
  it("reads the route's spelling, and nothing else", () => {
    expect(pickedMessageSort(params("sort=relevance"))).toEqual({
      sort: "relevance",
      order: "desc",
    });
    expect(pickedMessageSort(params("sort=date"))).toEqual({ sort: "date", order: "asc" });
    expect(pickedMessageSort(params("sort=-date"))).toEqual({ sort: "date", order: "desc" });
    expect(pickedMessageSort(params("sort=colour"))).toBeNull();
  });
});

describe("openedAt", () => {
  it("is a positive message id, or null", () => {
    expect(openedAt(params("at=42"))).toBe(42);
    expect(openedAt(params("at=0"))).toBeNull();
    expect(openedAt(params("at=4x"))).toBeNull();
    expect(openedAt(params(""))).toBeNull();
  });
});

describe("openedMatchedVersions", () => {
  it("is the earlier versions a result was found by, by their place in the message's list", () => {
    expect(openedMatchedVersions(params("matched=0,2"))).toEqual([0, 2]);
    expect(openedMatchedVersions(params("matched=1"))).toEqual([1]);
    expect(openedMatchedVersions(params(""))).toEqual([]);
    // A part that is not a place in a list is left out, and the rest kept.
    expect(openedMatchedVersions(params("matched=x,1,-2,1.5"))).toEqual([1]);
  });
});

describe("messagesSearch", () => {
  it("carries the search and the Messages list's parameters, and drops the filter", () => {
    expect(
      messagesSearch(params("q=photo&f=with%3A%2342&view=messages&sort=date&at=7"), {
        at: "9",
      }),
    ).toBe("?q=photo&view=messages&sort=date&at=9");
  });

  it("carries the earlier versions the opened result was found by", () => {
    expect(messagesSearch(params("q=noon&view=messages&at=7&matched=0"), {})).toBe(
      "?q=noon&view=messages&at=7&matched=0",
    );
    expect(messagesSearch(params("q=noon&at=7&matched=0"), { q: "noon" })).toBe(
      "?q=noon&at=7&matched=0",
    );
  });

  it("drops those versions with a new search, or with no message opened", () => {
    expect(messagesSearch(params("q=noon&at=7&matched=0"), { q: "pitied" })).toBe("?q=pitied&at=7");
    expect(messagesSearch(params("q=noon&at=7&matched=0"), { at: "" })).toBe("?q=noon");
    expect(messagesSearch(params("q=noon&at=7&matched=0"), { at: "9" })).toBe("?q=noon&at=9");
  });

  it("drops a parameter set to empty, and is empty when nothing is left", () => {
    expect(messagesSearch(params("q=photo&view=messages"), { view: "", q: "" })).toBe("");
  });
});
