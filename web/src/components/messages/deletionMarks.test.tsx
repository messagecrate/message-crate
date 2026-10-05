/** @vitest-environment jsdom */

import { cleanup, render, screen } from "@testing-library/react";
import type { ComponentType } from "react";
import { afterEach, describe, expect, it } from "vitest";
import { missingAttachmentChipLabel } from "../../lib/missingAttachmentLabel";
import { TimeZoneContext } from "../../lib/timeZone";
import type { Message } from "../../lib/types";
import { attachment, message as baseMessage, participant } from "../../test/apiShapes";
import type { MessageBubbleProps } from "./chatBubbleShared";
import DiscordBubble from "./DiscordBubble";
import ImessageBubble from "./ImessageBubble";
import InstagramBubble from "./InstagramBubble";
import SmsBubble from "./SmsBubble";
import WhatsAppBubble from "./WhatsAppBubble";

afterEach(() => {
  cleanup();
});

/**
 * Every source's bubble, with the source id it is drawn for and the name the
 * product gives that source.
 */
const BUBBLES: {
  name: string;
  Bubble: ComponentType<MessageBubbleProps>;
  source: string;
  service: string;
  label: string;
}[] = [
  {
    name: "Apple Messages",
    Bubble: ImessageBubble,
    source: "imessage",
    service: "iMessage",
    label: "Apple Messages",
  },
  {
    name: "SMS Backup & Restore",
    Bubble: SmsBubble,
    source: "sms-backup-restore",
    service: "sms",
    label: "SMS Backup & Restore",
  },
  {
    name: "WhatsApp",
    Bubble: WhatsAppBubble,
    source: "whatsapp",
    service: "whatsapp",
    label: "WhatsApp",
  },
  {
    name: "Discord",
    Bubble: DiscordBubble,
    source: "discord",
    service: "discord",
    label: "Discord",
  },
  {
    name: "Instagram",
    Bubble: InstagramBubble,
    source: "instagram",
    service: "instagram",
    label: "Instagram",
  },
];

/** An attachment the import kept without its file, so it draws as a chip and fetches nothing. */
const ATTACHMENT = attachment({
  original_name: "notes.pdf",
  mime_type: "application/pdf",
  missing_reason: "file_missing",
});
const ATTACHMENT_LABEL = missingAttachmentChipLabel(ATTACHMENT);

function message(partial: Partial<Message>): Message {
  return baseMessage({
    id: 7,
    service: "iMessage",
    guid: "g7",
    sender: "+15555550100",
    text: "See you at noon",
    conversation: {
      id: 1,
      chat_identifier: "+15555550100",
      conversation_type: "individual",
      is_group: false,
      group_title: null,
      label: null,
      participants: [participant({ identity: "+15555550100", name: "Ada" })],
    },
    ...partial,
  });
}

/** The bubble in UTC, so its time reads the same on every machine. */
function renderInUtc(Bubble: ComponentType<MessageBubbleProps>, m: Message) {
  return render(
    <TimeZoneContext.Provider value="UTC">
      <Bubble message={m} />
    </TimeZoneContext.Provider>,
  );
}

/** The element drawn as the message's bubble: the one holding `text`, with a dashed outline. */
function markedBubble(text: string): HTMLElement {
  const bubble = screen.getByText(text).closest(".border-dashed");
  if (!(bubble instanceof HTMLElement)) {
    throw new Error(`"${text}" is not inside a dashed outline`);
  }
  return bubble;
}

describe.each(BUBBLES)("$name bubble", ({ Bubble, source, service, label }) => {
  it("keeps the text of a message Deleted in the source app, muted in a dashed outline, and names the source beside its time", () => {
    renderInUtc(Bubble, message({ source, service, deletion: "deleted_in_source_app" }));

    const bubble = markedBubble("See you at noon");
    expect(bubble).toHaveClass("text-muted");
    const note = screen.getByText(`· Deleted in ${label}`);
    expect(note.parentElement?.textContent).toMatch(/3:04/);
    expect(screen.queryByText("Unsent")).not.toBeInTheDocument();
  });

  it("draws an Unsent message as an empty muted bubble reading Unsent, with its time", () => {
    renderInUtc(Bubble, message({ source, service, text: "", deletion: "unsent" }));

    const bubble = markedBubble("Unsent");
    expect(bubble).toHaveClass("text-muted");
    expect(bubble).toHaveTextContent(/^Unsent$/);
    expect(screen.getByText(/3:04/)).toBeInTheDocument();
    expect(screen.queryByText(/Deleted in/)).not.toBeInTheDocument();
  });

  it("reads Unsent in place of any text the backup kept for an Unsent message", () => {
    renderInUtc(Bubble, message({ source, service, deletion: "unsent" }));

    expect(markedBubble("Unsent")).toHaveTextContent(/^Unsent$/);
    expect(screen.queryByText("See you at noon")).not.toBeInTheDocument();
  });

  it("draws the attachments of a message Deleted in the source app inside its dashed outline, with no text", () => {
    renderInUtc(
      Bubble,
      message({
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
      message({ source, service, text: " ", deletion: "deleted_in_source_app" }),
    );

    expect(container.querySelector(".border-dashed")).toBeNull();
    expect(screen.getByText(`· Deleted in ${label}`)).toBeInTheDocument();
  });

  it("draws an Unsent message as the empty bubble alone, leaving out attachments and reactions", () => {
    renderInUtc(
      Bubble,
      message({
        source,
        service,
        text: "",
        attachments: [ATTACHMENT],
        tapbacks: [
          { emoji: null, is_from_me: false, kind: "loved", part_index: 0, sender: "+15555550100" },
        ],
        deletion: "unsent",
      }),
    );

    expect(markedBubble("Unsent")).toHaveTextContent(/^Unsent$/);
    expect(screen.queryByText(ATTACHMENT_LABEL)).not.toBeInTheDocument();
    expect(screen.queryByText(/❤️/)).not.toBeInTheDocument();
  });

  it("draws an unmarked message with no dashed outline and no note", () => {
    renderInUtc(Bubble, message({ source, service }));

    expect(screen.getByText("See you at noon").closest(".border-dashed")).toBeNull();
    expect(screen.queryByText(/Deleted in|Unsent/)).not.toBeInTheDocument();
  });
});
