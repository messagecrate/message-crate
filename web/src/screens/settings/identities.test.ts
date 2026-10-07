import { describe, expect, it } from "vitest";
import { type Identity, messagesPhrase } from "./identities";

function identity(direct: number, group: number, orphaned: number): Identity {
  return {
    address: "+15555550100",
    service: "phone",
    start_date: null,
    end_date: null,
    conversations: 0,
    direct_messages: direct,
    group_messages: group,
    orphaned_messages: orphaned,
  };
}

describe("messagesPhrase", () => {
  it("names all three kinds with commas and a final and", () => {
    expect(messagesPhrase(identity(1200, 30, 1))).toBe(
      `${(1200).toLocaleString()} direct messages, 30 group messages and 1 orphaned message`,
    );
  });

  it("joins two kinds with and, leaving out the one with none", () => {
    expect(messagesPhrase(identity(12, 0, 4))).toBe("12 direct messages and 4 orphaned messages");
  });

  it("names a single kind on its own", () => {
    expect(messagesPhrase(identity(0, 1, 0))).toBe("1 group message");
  });

  it("is null when there are no messages", () => {
    expect(messagesPhrase(identity(0, 0, 0))).toBeNull();
  });
});
