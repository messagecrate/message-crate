import { describe, expect, it } from "vitest";
import { messageSortFromParam, messageSortParam, pickedSortParam } from "./messageSearchSort";

describe("messageSortParam", () => {
  it("spells the sort as the route takes it", () => {
    expect(messageSortParam({ sort: "relevance", order: "desc" })).toBe("relevance");
    expect(messageSortParam({ sort: "relevance", order: "asc" })).toBe("relevance");
    expect(messageSortParam({ sort: "date", order: "asc" })).toBe("date");
    expect(messageSortParam({ sort: "date", order: "desc" })).toBe("-date");
  });
});

describe("messageSortFromParam", () => {
  it("reads the order the server reports, by its first key", () => {
    expect(messageSortFromParam("relevance")).toEqual({ sort: "relevance", order: "desc" });
    expect(messageSortFromParam("date")).toEqual({ sort: "date", order: "asc" });
    expect(messageSortFromParam("-date")).toEqual({ sort: "date", order: "desc" });
    expect(messageSortFromParam("relevance,-date")).toEqual({ sort: "relevance", order: "desc" });
    expect(messageSortFromParam("colour")).toBeNull();
    expect(messageSortFromParam(null)).toBeNull();
  });
});

describe("pickedSortParam", () => {
  it("keeps a Date order, and nothing for Relevance, which is the server's own order", () => {
    expect(pickedSortParam({ sort: "date", order: "asc" })).toBe("date");
    expect(pickedSortParam({ sort: "date", order: "desc" })).toBe("-date");
    expect(pickedSortParam({ sort: "relevance", order: "desc" })).toBeNull();
  });
});
