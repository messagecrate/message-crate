/** @vitest-environment jsdom */

import { cleanup, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { missingAttachmentChipLabel } from "../../lib/missingAttachmentLabel";
import { attachment, imessageMessage } from "../../test/apiShapes";
import { BUBBLES, renderBubbleInUtc as renderInUtc } from "../../test/bubbles";

afterEach(() => {
  cleanup();
});

/** An attachment the import kept without its file, so it draws as a chip and fetches nothing. */
const ATTACHMENT = attachment({
  original_name: "notes.pdf",
  mime_type: "application/pdf",
  missing_reason: "file_missing",
});
const ATTACHMENT_LABEL = missingAttachmentChipLabel(ATTACHMENT);

/** The text of a message whose bubble a test looks for, or checks is gone. */
const KEPT_TEXT = "See you at noon";

/** A reaction, drawn as ❤️ by the bubbles that draw reactions. The sender only names who reacted. */
const LOVED = { emoji: null, is_from_me: false, kind: "loved", part_index: 0, sender: "+1555" };

/** The element drawn as the message's bubble: the one holding `text`, with a dashed outline. */
function markedBubble(text: string): HTMLElement {
  const bubble = screen.getByText(text).closest(".border-dashed");
  if (!(bubble instanceof HTMLElement)) {
    throw new Error(`"${text}" is not inside a dashed outline`);
  }
  return bubble;
}

describe.each(BUBBLES)("$label bubble", ({ Bubble, source, service, label, drawsReactions }) => {
  it("keeps the text of a message Deleted in the source app, muted in a dashed outline, and names the source beside its time", () => {
    renderInUtc(
      Bubble,
      imessageMessage({ source, service, text: KEPT_TEXT, deletion: "deleted_in_source_app" }),
    );

    const bubble = markedBubble(KEPT_TEXT);
    expect(bubble).toHaveClass("text-muted");
    const note = screen.getByText(`· Deleted in ${label}`);
    expect(note.parentElement?.textContent).toMatch(/3:04/);
    expect(screen.queryByText("Unsent")).not.toBeInTheDocument();
  });

  it("draws an Unsent message as an empty muted bubble reading Unsent, with its time", () => {
    renderInUtc(Bubble, imessageMessage({ source, service, text: "", deletion: "unsent" }));

    const bubble = markedBubble("Unsent");
    expect(bubble).toHaveClass("text-muted");
    expect(bubble).toHaveTextContent(/^Unsent$/);
    expect(screen.getByText(/3:04/)).toBeInTheDocument();
    expect(screen.queryByText(/Deleted in/)).not.toBeInTheDocument();
  });

  it("reads Unsent in place of any text the backup kept for an Unsent message", () => {
    renderInUtc(Bubble, imessageMessage({ source, service, text: KEPT_TEXT, deletion: "unsent" }));

    expect(markedBubble("Unsent")).toHaveTextContent(/^Unsent$/);
    expect(screen.queryByText(KEPT_TEXT)).not.toBeInTheDocument();
  });

  it("draws the attachments of a message Deleted in the source app inside its dashed outline, with no text", () => {
    renderInUtc(
      Bubble,
      imessageMessage({
        source,
        service,
        text: "",
        attachments: [ATTACHMENT],
        deletion: "deleted_in_source_app",
      }),
    );

    expect(markedBubble(ATTACHMENT_LABEL)).toHaveClass("text-muted");
    expect(screen.getByText(`· Deleted in ${label}`)).toBeInTheDocument();
  });

  it("draws no empty outline for a message Deleted in the source app that kept nothing, only the note", () => {
    const { container } = renderInUtc(
      Bubble,
      imessageMessage({ source, service, text: " ", deletion: "deleted_in_source_app" }),
    );

    expect(container.querySelector(".border-dashed")).toBeNull();
    expect(screen.getByText(`· Deleted in ${label}`)).toBeInTheDocument();
  });

  it("draws an Unsent message as the empty bubble alone, leaving out its attachments", () => {
    renderInUtc(
      Bubble,
      imessageMessage({ source, service, text: "", attachments: [ATTACHMENT], deletion: "unsent" }),
    );

    expect(markedBubble("Unsent")).toHaveTextContent(/^Unsent$/);
    expect(screen.queryByText(ATTACHMENT_LABEL)).not.toBeInTheDocument();
  });

  it(`${drawsReactions ? "draws" : "draws no"} reactions on an unmarked message`, () => {
    renderInUtc(Bubble, imessageMessage({ source, service, text: KEPT_TEXT, tapbacks: [LOVED] }));

    expect(screen.queryByText(/❤️/) !== null).toBe(drawsReactions);
  });

  it("draws an unmarked message with no dashed outline and no note", () => {
    renderInUtc(Bubble, imessageMessage({ source, service, text: KEPT_TEXT }));

    expect(screen.getByText(KEPT_TEXT).closest(".border-dashed")).toBeNull();
    expect(screen.queryByText(/Deleted in|Unsent/)).not.toBeInTheDocument();
  });
});

/**
 * Only the bubbles that draw reactions can show whether an Unsent message
 * leaves its reactions out; the "draws reactions" test above pins which ones.
 */
describe.each(BUBBLES.filter((b) => b.drawsReactions))(
  "$label bubble",
  ({ Bubble, source, service }) => {
    it("draws an Unsent message without its reactions", () => {
      renderInUtc(
        Bubble,
        imessageMessage({ source, service, text: "", tapbacks: [LOVED], deletion: "unsent" }),
      );

      expect(markedBubble("Unsent")).toHaveTextContent(/^Unsent$/);
      expect(screen.queryByText(/❤️/)).not.toBeInTheDocument();
    });
  },
);
