/** @vitest-environment jsdom */

import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { fetchAssetObjectUrl } from "../../lib/serverApi";
import type { Message } from "../../lib/types";
import { attachment, imessageMessage, participant } from "../../test/apiShapes";
import {
  installIntersectionObserver,
  observersWithin,
  scrollNear,
} from "../../test/intersectionObserver";
import { renderWithProviders } from "../../test/providers";
import ConversationView from "./ConversationView";

vi.mock("../../lib/serverApi", () => ({
  fetchAsset: vi.fn(),
  fetchAssetObjectUrl: vi.fn(),
  createMediaLink: vi.fn(),
}));

afterEach(() => {
  cleanup();
});

const baseProps = {
  loading: false,
  error: null as unknown,
  findTerm: "",
  highlightId: null as number | null,
  isGroup: false,
  hasOlder: false,
  hasNewer: false,
  loadingOlder: false,
  loadingNewer: false,
  onLoadOlder: vi.fn(),
  onLoadNewer: vi.fn(),
  landing: { seq: 0, to: "bottom" as const },
  onAttachmentClick: vi.fn(),
};

describe("ConversationView", () => {
  it("shows a loading state", () => {
    render(<ConversationView {...baseProps} messages={[]} loading={true} />);
    expect(screen.getByText("Loading…")).toBeInTheDocument();
  });

  it("shows the server's error sentence when the query failed", () => {
    render(<ConversationView {...baseProps} messages={[]} error={new Error("server is down")} />);
    expect(screen.getByText("server is down")).toBeInTheDocument();
  });

  it("falls back to a generic message when the error carries no message", () => {
    render(<ConversationView {...baseProps} messages={[]} error={"not an Error"} />);
    expect(screen.getByText("Could not load messages.")).toBeInTheDocument();
  });

  it("shows an empty state when the conversation has no messages", () => {
    render(<ConversationView {...baseProps} messages={[]} />);
    expect(screen.getByText("No messages in this conversation")).toBeInTheDocument();
  });

  it("renders messages when there are some", () => {
    render(<ConversationView {...baseProps} messages={[imessageMessage()]} />);
    expect(screen.getByText("hi")).toBeInTheDocument();
    expect(screen.queryByText("No messages in this conversation")).not.toBeInTheDocument();
  });

  it("names a group's sender at the start of a run only", () => {
    const group = {
      id: 1,
      chat_identifier: "x",
      conversation_type: "group",
      is_group: true,
      group_title: null,
      label: null,
      participants: [
        participant({ identity: "+15555550101", name: "Ada" }),
        participant({ identity: "+15555550102", name: "Bo" }),
      ],
    };
    const from = (id: number, sender: string, timestamp: string) =>
      imessageMessage({ id, sender, timestamp, text: `m${id}`, conversation: group });
    render(
      <ConversationView
        {...baseProps}
        isGroup={true}
        messages={[
          from(1, "+15555550101", "2026-08-11T15:00:00Z"),
          from(2, "+15555550101", "2026-08-11T15:05:00Z"),
          from(3, "+15555550102", "2026-08-11T15:06:00Z"),
        ]}
      />,
    );
    expect(screen.getAllByText("Ada")).toHaveLength(1);
    expect(screen.getAllByText("Bo")).toHaveLength(1);
  });

  it("says older messages load by scrolling, with no Previous or Next", () => {
    render(<ConversationView {...baseProps} messages={[imessageMessage()]} hasOlder={true} />);
    expect(screen.getByText("Scroll up for older messages")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Previous" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Next" })).toBeNull();
  });
});

/**
 * Opening a conversation at a message scrolls the conversation to it and nothing
 * else: in a narrow window the row under the header scrolls sideways (#1722),
 * and moving it would pan the navigation panel and the list out of view.
 */
describe("ConversationView landing on a message", () => {
  afterEach(() => {
    vi.restoreAllMocks();
    Reflect.deleteProperty(Element.prototype, "scrollIntoView");
  });

  it.each([
    // The message's top, 400 px below the conversation's top, lands at the top.
    ["start", 400],
    // A 40 px message in a 300 px conversation lands 130 px down.
    ["center", 270],
  ] as const)("scrolls only the conversation to a message at %s", (align, scrollTop) => {
    const scrollIntoView = vi.fn();
    Element.prototype.scrollIntoView = scrollIntoView;
    vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(300);
    vi.spyOn(Element.prototype, "getBoundingClientRect").mockImplementation(function (
      this: Element,
    ) {
      return (this.id === "row-2" ? { top: 500, height: 40 } : { top: 100, height: 0 }) as DOMRect;
    });

    render(
      <ConversationView
        {...baseProps}
        messages={[
          imessageMessage({ id: 1, timestamp: "2026-08-11T15:00:00Z", text: "m1" }),
          imessageMessage({ id: 2, timestamp: "2026-08-11T15:05:00Z", text: "m2" }),
          imessageMessage({ id: 3, timestamp: "2026-08-11T15:06:00Z", text: "m3" }),
        ]}
        landing={{ seq: 1, to: { id: 2, align } }}
      />,
    );

    const view = document.getElementById("row-2")?.closest(".overflow-auto") as HTMLElement;
    expect(view.scrollTop).toBe(scrollTop);
    expect(scrollIntoView).not.toHaveBeenCalled();
  });
});

/**
 * Scrolling a long conversation loads only the Thumbnails of the messages
 * that come near the screen, measured against the conversation's own scroll area
 * (`docs/architecture/media.md`, rule 5). Measured against the window, the
 * margin would not reach past the conversation's edge, and a Thumbnail would start
 * loading only once it was already on screen.
 */
describe("ConversationView and attachments", () => {
  function withPhoto(id: number, sha256: string): Message {
    return imessageMessage({
      id,
      text: null,
      attachments: [
        attachment({
          original_name: `${sha256}.jpg`,
          mime_type: "image/jpeg",
          sha256,
          thumbnail_mime_type: "image/jpeg",
        }),
      ],
    });
  }

  it("fetches the Thumbnail of the message scrolled near the screen, and nothing else", async () => {
    installIntersectionObserver();
    vi.mocked(fetchAssetObjectUrl).mockImplementation(
      async (sha, options) => `blob:${options?.version}-${sha}`,
    );
    const { container } = renderWithProviders(
      <ConversationView {...baseProps} messages={[withPhoto(1, "aaa"), withPhoto(2, "bbb")]} />,
    );
    const row = container.querySelector("#row-2");
    if (!row) throw new Error("no row for message 2");
    expect(fetchAssetObjectUrl).not.toHaveBeenCalled();
    const scrollArea = container.firstElementChild;
    expect(observersWithin(row)).toEqual([{ root: scrollArea, rootMargin: "800px 0px" }]);

    scrollNear(row);

    expect(await screen.findByRole("img", { name: "bbb.jpg" })).toHaveAttribute(
      "src",
      "blob:thumbnail-bbb",
    );
    expect(vi.mocked(fetchAssetObjectUrl).mock.calls.map(([sha, o]) => [sha, o?.version])).toEqual([
      ["bbb", "thumbnail"],
    ]);
  });
});
