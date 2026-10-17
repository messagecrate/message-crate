import { describe, expect, it } from "vitest";
import { conversationName } from "./conversationName";

const alice = { name: "Alice" };
const bob = { name: "Bob" };

describe("conversationName", () => {
  it("uses the title when there is one", () => {
    expect(
      conversationName({ shown_title: " Book club ", is_group: true, participants: [alice] }),
    ).toBe("Book club");
  });

  it("names a one-to-one conversation after the other person", () => {
    expect(
      conversationName({ shown_title: null, is_group: false, participants: [alice, bob] }),
    ).toBe("Alice");
  });

  it("names an untitled group after every participant", () => {
    expect(conversationName({ shown_title: "", is_group: true, participants: [alice, bob] })).toBe(
      "Alice, Bob",
    );
  });

  it("names a conversation with yourself by the title the server gives it", () => {
    // The server titles it with the account's display name, or the address;
    // it has no participants to fall back on.
    expect(conversationName({ shown_title: "Sam Holder", is_group: false, participants: [] })).toBe(
      "Sam Holder",
    );
  });

  it("says (unknown) when there is nobody to name", () => {
    expect(conversationName({ shown_title: null, is_group: false, participants: [] })).toBe(
      "(unknown)",
    );
    expect(conversationName({ shown_title: null, is_group: true, participants: [] })).toBe(
      "(unknown)",
    );
  });
});
