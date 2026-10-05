/** @vitest-environment jsdom */

import { cleanup, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import type { Message } from "../../lib/types";
import { BUBBLES, bubbleMessage, renderBubbleInUtc } from "../../test/bubbles";
import { setupUser } from "../../test/user";

afterEach(() => {
  cleanup();
});

/**
 * A message edited twice: sent as "See you at noon" at 15:04, edited at 15:06
 * to "See you at half past noon", and later to its final text.
 */
function edited(source: string, service: string, partial: Partial<Message> = {}): Message {
  return bubbleMessage({
    source,
    service,
    text: "See you at one",
    earlier_versions: [
      { part_index: 0, text: "See you at noon", edited_at: "2026-08-11T15:04:00Z", matched: false },
      {
        part_index: 0,
        text: "See you at half past noon",
        edited_at: "2026-08-11T15:06:00Z",
        matched: false,
      },
    ],
    ...partial,
  });
}

/** The earlier versions as drawn, one list item each, oldest first. */
function versionItems(): HTMLElement[] {
  return within(screen.getByRole("list", { name: "Earlier versions" })).getAllByRole("listitem");
}

describe.each(BUBBLES)("$name bubble", ({ Bubble, source, service }) => {
  it("reads Edited beside the time of an edited message and keeps its earlier versions closed", () => {
    renderBubbleInUtc(Bubble, edited(source, service));

    const control = screen.getByRole("button", { name: "Edited" });
    expect(control).toHaveAttribute("aria-expanded", "false");
    expect(control.closest("div")?.textContent).toMatch(/3:04.*· Edited/);
    expect(screen.getByText("See you at one")).toBeInTheDocument();
    expect(screen.queryByRole("list", { name: "Earlier versions" })).not.toBeInTheDocument();
    expect(screen.queryByText("See you at noon")).not.toBeInTheDocument();
    expect(screen.queryByText("Matched an earlier version")).not.toBeInTheDocument();
  });

  it("opens the earlier versions under the bubble, each with its time, and closes them again", async () => {
    const user = setupUser();
    renderBubbleInUtc(Bubble, edited(source, service));
    const control = screen.getByRole("button", { name: "Edited" });

    await user.click(control);

    expect(control).toHaveAttribute("aria-expanded", "true");
    const [first, second] = versionItems();
    expect(first).toHaveTextContent("See you at noon");
    expect(first).toHaveTextContent("Aug 11, 3:04 PM");
    expect(second).toHaveTextContent("See you at half past noon");
    expect(second).toHaveTextContent("Aug 11, 3:06 PM");
    expect(screen.queryByText("Matched an earlier version")).not.toBeInTheDocument();
    expect(screen.queryByText(/./, { selector: "[data-matched]" })).not.toBeInTheDocument();

    await user.click(control);

    expect(control).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByText("See you at noon")).not.toBeInTheDocument();
  });

  it("opens a message a search found only by an earlier version, with that version highlighted", () => {
    renderBubbleInUtc(
      Bubble,
      edited(source, service, {
        matched_earlier_version: true,
        earlier_versions: [
          {
            part_index: 0,
            text: "See you at noon",
            edited_at: "2026-08-11T15:04:00Z",
            matched: true,
          },
          {
            part_index: 0,
            text: "See you at half past noon",
            edited_at: "2026-08-11T15:06:00Z",
            matched: false,
          },
        ],
      }),
    );

    expect(screen.getByRole("button", { name: "Edited" })).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByText("Matched an earlier version")).toBeInTheDocument();
    const [first, second] = versionItems();
    expect(first).toHaveAttribute("data-matched");
    expect(first).toHaveClass("bg-search-mark");
    expect(second).not.toHaveAttribute("data-matched");
  });

  it("keeps closed an edited message a search found by its final text", () => {
    // A hit by the final text carries no version marked, even when an earlier
    // version holds the word too.
    renderBubbleInUtc(Bubble, edited(source, service, { matched_earlier_version: false }));

    expect(screen.getByRole("button", { name: "Edited" })).toHaveAttribute(
      "aria-expanded",
      "false",
    );
    expect(screen.queryByText("Matched an earlier version")).not.toBeInTheDocument();
  });

  it("draws no Edited control on a message never edited", () => {
    renderBubbleInUtc(Bubble, bubbleMessage({ source, service }));

    expect(screen.queryByRole("button", { name: "Edited" })).not.toBeInTheDocument();
    expect(screen.queryByText(/Edited/)).not.toBeInTheDocument();
  });

  it("reads Edited after the note of an edited message Deleted in the source app", () => {
    renderBubbleInUtc(Bubble, edited(source, service, { deletion: "deleted_in_source_app" }));

    const control = screen.getByRole("button", { name: "Edited" });
    expect(control.closest("div")?.textContent).toMatch(/Deleted in .*· Edited/);
  });
});
