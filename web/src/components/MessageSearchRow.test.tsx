/** @vitest-environment jsdom */

import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import type { Message } from "../lib/types";
import { attachment, message as baseMessage, participant } from "../test/apiShapes";
import { inTimeZone } from "../test/timeZone";
import MessageSearchRow from "./MessageSearchRow";

afterEach(cleanup);

/** The account's zone every row here renders in. */
const ZONE = "America/New_York";

function message(over: Partial<Message> = {}): Message {
  return baseMessage({
    id: 10,
    guid: "g10",
    // 03:30 UTC on 2 January is still 1 January in New York.
    timestamp: "2024-01-02T03:30:00Z",
    sender: "+15555550100",
    text: "Here is the photo from the dentist",
    conversation: {
      id: 1,
      chat_identifier: "chat1",
      conversation_type: "group",
      is_group: true,
      group_title: "Family",
      label: "Family",
      participants: [
        participant({ name: "Alice", identity: "+15555550100" }),
        participant({ name: "Bob", identity: "+15555550120" }),
      ],
    },
    ...over,
  });
}

function renderRow(m: Message, terms = [{ text: "photo", prefix: false }]) {
  return render(
    inTimeZone(
      ZONE,
      <MessageSearchRow message={m} terms={terms} isSelected={false} onClick={() => {}} />,
    ),
  );
}

describe("MessageSearchRow", () => {
  it("shows the conversation, the day in the account's zone, the sender, and the matching word in bold", () => {
    const m = message();
    renderRow(m);
    const row = screen.getByRole("button");
    expect(row).toHaveTextContent("Family");
    expect(row).toHaveTextContent(
      new Date(m.timestamp).toLocaleDateString([], {
        timeZone: ZONE,
        year: "numeric",
        month: "short",
        day: "numeric",
      }),
    );
    expect(row).toHaveTextContent("Alice: Here is the photo from the dentist");
    const bold = row.querySelectorAll("strong");
    expect([...bold].map((b) => b.textContent)).toEqual(["photo"]);
  });

  it('says "You" for a sent message, and counts its attachments', () => {
    renderRow(
      message({
        is_from_me: true,
        sender: null,
        attachments: [
          attachment({ original_name: "a.jpg" }),
          attachment({ original_name: "b.jpg" }),
        ],
      }),
    );
    const row = screen.getByRole("button");
    expect(row).toHaveTextContent("You:");
    expect(screen.getByTitle("2 attachments")).toHaveTextContent("📎 2");
  });

  it("writes a large attachment count with a separator", () => {
    renderRow(
      message({
        attachments: Array.from({ length: 1234 }, (_, i) =>
          attachment({ original_name: `${i}.jpg` }),
        ),
      }),
    );
    expect(screen.getByTitle("1,234 attachments")).toBeInTheDocument();
  });

  it("shows no sender for a received message that names none", () => {
    renderRow(message({ sender: null }));
    const row = screen.getByRole("button");
    expect(row).not.toHaveTextContent("Unknown");
    expect(row).not.toHaveTextContent(":");
  });

  it("shows the attachments' names for a message with no text, with a matching name in bold", () => {
    renderRow(message({ text: null, attachments: [attachment({ original_name: "photo 1.jpg" })] }));
    const row = screen.getByRole("button");
    expect(row).toHaveTextContent("Alice: photo 1.jpg");
    expect(row.querySelector("strong")?.textContent).toBe("photo");
  });

  it("keeps the text of a message Deleted in the source app and notes the source by its name", () => {
    renderRow(message({ deletion: "deleted_in_source_app" }));
    const row = screen.getByRole("button");
    expect(row).toHaveTextContent("Alice: Here is the photo from the dentist");
    const note = screen.getByText("Deleted in Apple Messages");
    expect(note.closest(".text-muted")).not.toBeNull();
    expect(row).not.toHaveTextContent("Unsent");
  });

  it('shows "Unsent" in place of the text of an Unsent message, in the muted colour', () => {
    renderRow(
      message({
        deletion: "unsent",
        text: null,
        attachments: [attachment({ original_name: "photo 1.jpg" })],
      }),
    );
    const row = screen.getByRole("button");
    expect(row).toHaveTextContent("Alice: Unsent");
    expect(row).not.toHaveTextContent("photo 1.jpg");
    expect(row).not.toHaveTextContent("📎");
    expect(row).not.toHaveTextContent("Deleted in");
    expect(screen.getByText("Unsent").closest(".text-muted")).not.toBeNull();
  });

  it("carries no mark for an unmarked message", () => {
    renderRow(message());
    const row = screen.getByRole("button");
    expect(row).not.toHaveTextContent("Deleted in");
    expect(row).not.toHaveTextContent("Unsent");
  });

  it('quotes the newest earlier version a search found it by under the final text, as "Earlier version:" with the word in bold', () => {
    renderRow(
      message({
        text: "Those who do not complain are never pitied.",
        matched_earlier_version: true,
        earlier_versions: [
          {
            part_index: 0,
            text: "An imprudent first draft",
            matched: true,
            edited_at: "2024-01-01T10:00:00Z",
          },
          {
            part_index: 0,
            text: "So imprudent a match on both sides!",
            matched: true,
            edited_at: "2024-01-01T11:00:00Z",
          },
          {
            part_index: 0,
            text: "A version with no match",
            matched: false,
            edited_at: "2024-01-01T12:00:00Z",
          },
        ],
      }),
      [{ text: "imprudent", prefix: false }],
    );
    const row = screen.getByRole("button");
    expect(row).toHaveTextContent("Alice: Those who do not complain are never pitied.");
    const line = screen.getByText(/^Earlier version:/);
    expect(line).toHaveTextContent("Earlier version: So imprudent a match on both sides!");
    expect(line).toHaveClass("text-muted");
    expect([...line.querySelectorAll("strong")].map((b) => b.textContent)).toEqual(["imprudent"]);
    expect(row).not.toHaveTextContent("first draft");
  });

  it("cuts the earlier version around its matching word the way the final text is cut", () => {
    const words = "one two three four five six seven eight nine ten eleven";
    renderRow(
      message({
        text: "Nothing here",
        matched_earlier_version: true,
        earlier_versions: [
          { part_index: 0, text: `${words} imprudent end`, matched: true, edited_at: null },
        ],
      }),
      [{ text: "imprudent", prefix: false }],
    );
    const line = screen.getByText(/^Earlier version:/);
    expect(line.textContent).toMatch(/^Earlier version: …\S.* imprudent end$/);
    expect(line.textContent).not.toContain("one two");
  });

  it("shows no earlier version line for a hit its final text holds the word of, only for one found by an earlier version", () => {
    const version = (matched: boolean) => ({
      part_index: 0,
      text: "Here is a photo",
      matched,
      edited_at: "2024-01-01T10:00:00Z",
    });
    renderRow(message({ earlier_versions: [version(false)] }));
    const byFinalText = screen.getByRole("button");
    expect(byFinalText).not.toHaveTextContent("Earlier version");
    expect([...byFinalText.querySelectorAll("strong")].map((b) => b.textContent)).toEqual([
      "photo",
    ]);
    cleanup();

    renderRow(
      message({
        text: "Here is the picture",
        matched_earlier_version: true,
        earlier_versions: [version(true)],
      }),
    );
    expect(screen.getByRole("button")).toHaveTextContent("Earlier version: Here is a photo");
  });
  it("takes the later of two versions of one part as the newer, even when only the older has a time", () => {
    renderRow(
      message({
        text: "Nothing here",
        matched_earlier_version: true,
        earlier_versions: [
          {
            part_index: 0,
            text: "imprudent at first",
            matched: true,
            edited_at: "2024-01-01T10:00:00Z",
          },
          { part_index: 0, text: "imprudent at last", matched: true, edited_at: null },
        ],
      }),
      [{ text: "imprudent", prefix: false }],
    );
    expect(screen.getByText(/^Earlier version:/)).toHaveTextContent(
      "Earlier version: imprudent at last",
    );
  });

  it("quotes a version for each searched word only an older matched version holds, newest first", () => {
    renderRow(
      message({
        text: "Nothing here",
        matched_earlier_version: true,
        earlier_versions: [
          { part_index: 0, text: "alpha first", matched: true, edited_at: "2024-01-01T10:00:00Z" },
          { part_index: 0, text: "alpha again", matched: true, edited_at: "2024-01-01T11:00:00Z" },
          { part_index: 0, text: "beta last", matched: true, edited_at: "2024-01-01T12:00:00Z" },
        ],
      }),
      [
        { text: "alpha", prefix: false },
        { text: "beta", prefix: false },
      ],
    );
    expect(screen.getAllByText(/^Earlier version:/).map((line) => line.textContent)).toEqual([
      "Earlier version: beta last",
      "Earlier version: alpha again",
    ]);
  });

  it("takes the newest matched version by time across parts, whatever an undated part between them says", () => {
    renderRow(
      message({
        text: "Nothing here",
        matched_earlier_version: true,
        earlier_versions: [
          { part_index: 0, text: "alpha at ten", matched: true, edited_at: "2024-01-01T10:00:00Z" },
          { part_index: 1, text: "alpha undated", matched: true, edited_at: null },
          {
            part_index: 2,
            text: "alpha at nine",
            matched: true,
            edited_at: "2024-01-01T09:00:00Z",
          },
        ],
      }),
      [{ text: "alpha", prefix: false }],
    );
    expect(screen.getAllByText(/^Earlier version:/).map((line) => line.textContent)).toEqual([
      "Earlier version: alpha at ten",
    ]);
  });

  it("cuts a version quoted for a word the newest lacks around that word, with every searched word in bold", () => {
    const filler = "and then some more words go here ".repeat(10);
    renderRow(
      message({
        text: "Nothing here",
        matched_earlier_version: true,
        earlier_versions: [
          {
            part_index: 0,
            text: `foo ${filler}bar`,
            matched: true,
            edited_at: "2024-01-01T10:00:00Z",
          },
          { part_index: 0, text: "foo again", matched: true, edited_at: "2024-01-01T11:00:00Z" },
        ],
      }),
      [
        { text: "foo", prefix: false },
        { text: "bar", prefix: false },
      ],
    );
    const lines = screen.getAllByText(/^Earlier version:/);
    expect(lines).toHaveLength(2);
    expect(lines[0]).toHaveTextContent("Earlier version: foo again");
    expect(lines[1].textContent).toMatch(/^Earlier version: …/);
    expect(lines[1].textContent).not.toContain("foo");
    expect(lines[1].textContent).toMatch(/ bar$/);
    expect([...lines[1].querySelectorAll("strong")].map((b) => b.textContent)).toEqual(["bar"]);
  });

  it("quotes no older version for a searched word the final text already shows", () => {
    renderRow(
      message({
        text: "foo now",
        matched_earlier_version: true,
        earlier_versions: [
          { part_index: 0, text: "foo first", matched: true, edited_at: "2024-01-01T10:00:00Z" },
          { part_index: 0, text: "bar later", matched: true, edited_at: "2024-01-01T11:00:00Z" },
        ],
      }),
      [
        { text: "foo", prefix: false },
        { text: "bar", prefix: false },
      ],
    );
    expect(screen.getAllByText(/^Earlier version:/).map((line) => line.textContent)).toEqual([
      "Earlier version: bar later",
    ]);
  });
});
