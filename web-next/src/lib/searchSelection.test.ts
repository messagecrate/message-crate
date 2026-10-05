import assert from "node:assert/strict";
import { describe, it } from "node:test";
import type { SearchConversationHit } from "@/lib/search";
import {
  applySearchResultRangeSelect,
  applySearchRangeSelect,
  orderedSearchContactIds,
  orderedSearchResultKeys,
  searchResultKey,
  selectedSearchResultGroups,
} from "./searchSelection";

function hit(
  partial: Partial<SearchConversationHit> &
    Pick<SearchConversationHit, "conversationId">,
): SearchConversationHit {
  return {
    conversationType: "individual",
    contactId: null,
    title: "t",
    chatIdentifier: "c",
    matchCount: 1,
    dateStart: null,
    dateEnd: null,
    topMatch: null,
    ...partial,
  };
}

describe("orderedSearchContactIds", () => {
  it("keeps first occurrence of each contact and skips groups/unassigned", () => {
    const hits = [
      hit({ conversationId: 1, contactId: 10 }),
      hit({ conversationId: 2, conversationType: "group", contactId: null }),
      hit({ conversationId: 3, contactId: null }),
      hit({ conversationId: 4, contactId: 20 }),
      hit({ conversationId: 5, contactId: 10 }),
    ];
    assert.deepEqual(orderedSearchContactIds(hits), [10, 20]);
  });
});

describe("applySearchRangeSelect", () => {
  const ordered = [10, 20, 30, 40];

  it("selects only the clicked contact when there is no anchor", () => {
    assert.deepEqual([...applySearchRangeSelect(ordered, 30, null)], [30]);
  });

  it("selects the contiguous range from anchor to click", () => {
    assert.deepEqual(
      [...applySearchRangeSelect(ordered, 40, 20)].sort((a, b) => a - b),
      [20, 30, 40],
    );
  });

  it("works when click is above the anchor", () => {
    assert.deepEqual(
      [...applySearchRangeSelect(ordered, 10, 30)].sort((a, b) => a - b),
      [10, 20, 30],
    );
  });

  it("returns empty when the click is not in the ordered list", () => {
    assert.deepEqual([...applySearchRangeSelect(ordered, 99, 20)], []);
  });
});

describe("message search result selection", () => {
  const hits = [
    hit({ conversationId: 1, chatIdentifier: "+15550101" }),
    hit({
      conversationId: 2,
      conversationType: "group",
      chatIdentifier: "group-a",
    }),
    hit({ conversationId: 3, chatIdentifier: "+15550101" }),
    hit({ conversationId: 4, chatIdentifier: "+15550102" }),
  ];

  it("keys direct threads by handle and groups by conversation", () => {
    assert.equal(searchResultKey(hits[0]!), "direct:+15550101");
    assert.equal(searchResultKey(hits[1]!), "group:2");
    assert.deepEqual(orderedSearchResultKeys(hits), [
      "direct:+15550101",
      "group:2",
      "direct:+15550102",
    ]);
  });

  it("selects a mixed contiguous range", () => {
    const ordered = orderedSearchResultKeys(hits);
    assert.deepEqual(
      [
        ...applySearchResultRangeSelect(
          ordered,
          "direct:+15550102",
          "direct:+15550101",
        ),
      ],
      ["direct:+15550101", "group:2", "direct:+15550102"],
    );
  });

  it("groups duplicate direct rows into one selected thread", () => {
    const groups = selectedSearchResultGroups(
      hits,
      new Set(["direct:+15550101", "group:2"]),
    );
    assert.deepEqual(groups, [
      {
        key: "direct:+15550101",
        conversationType: "individual",
        chatIdentifier: "+15550101",
        title: "t",
        conversationIds: [1, 3],
      },
      {
        key: "group:2",
        conversationType: "group",
        chatIdentifier: "group-a",
        title: "t",
        conversationIds: [2],
      },
    ]);
  });
});
