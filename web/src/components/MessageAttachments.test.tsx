/** @vitest-environment jsdom */

import { fireEvent, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { CANNOT_PLAY_HERE, NO_PLAYABLE_COPY, STOPPED } from "../hooks/useStreamedMedia";
import { saveFile } from "../lib/saveFile";
import { createMediaLink, fetchAsset, fetchAssetObjectUrl } from "../lib/serverApi";
import type { Message, MessageAttachment } from "../lib/types";
import { attachment, message as baseMessage, participant } from "../test/apiShapes";
import { installIntersectionObserver, scrollNear } from "../test/intersectionObserver";
import { renderWithProviders } from "../test/providers";
import { setupUser } from "../test/user";
import MessageAttachments from "./MessageAttachments";

vi.mock("../lib/serverApi", () => ({
  fetchAsset: vi.fn(),
  fetchAssetObjectUrl: vi.fn(),
  createMediaLink: vi.fn(),
}));
vi.mock("../lib/saveFile", () => ({ saveDownload: vi.fn(), saveFile: vi.fn() }));

const LINK = {
  url: "http://server/v1/assets/x?media_link=l",
  preview_url: "http://server/v1/assets/x/preview?media_link=l",
  thumbnail_url: "http://server/v1/assets/x/thumbnail?media_link=l",
  expires_at: "2026-10-04T13:00:00Z",
};

beforeEach(() => {
  installIntersectionObserver();
  vi.mocked(fetchAssetObjectUrl).mockImplementation(
    async (sha, options) => `blob:${options?.version ?? "original"}-${sha}`,
  );
  vi.mocked(createMediaLink).mockResolvedValue(LINK);
});

afterEach(() => {
  vi.clearAllMocks();
});

function message(attachments: MessageAttachment[]): Message {
  return baseMessage({
    service: "iMessage",
    sender: "+1555",
    attachments,
    conversation: {
      id: 1,
      chat_identifier: "x",
      conversation_type: "individual",
      is_group: false,
      group_title: null,
      label: null,
      participants: [participant({ identity: "+1555", name: "Ada" })],
    },
  });
}

/** Each fetch of attachment bytes: whose, and which version. */
function fetches(): { sha256: string; version: string | undefined }[] {
  return vi
    .mocked(fetchAssetObjectUrl)
    .mock.calls.map(([sha256, options]) => ({ sha256, version: options?.version }));
}

const photo = attachment({
  original_name: "cat.jpg",
  mime_type: "image/jpeg",
  sha256: "aaa",
  thumbnail_mime_type: "image/jpeg",
});

const heic = attachment({
  original_name: "IMG_0001.heic",
  mime_type: "image/heic",
  sha256: "bbb",
  preview_mime_type: "image/jpeg",
  thumbnail_mime_type: "image/jpeg",
});

/**
 * A conversation holds thousands of photos, and a phone photo is megabytes.
 * A photo fetches nothing until its message comes near the screen, and then
 * only its Thumbnail, never the original or the Preview
 * (`docs/architecture/media.md`, rule 5).
 */
describe("a photo in the conversation", () => {
  it("fetches nothing until its message comes near the screen, then only its Thumbnail", async () => {
    const { container } = renderWithProviders(
      <MessageAttachments message={message([photo, heic])} />,
    );
    expect(fetchAssetObjectUrl).not.toHaveBeenCalled();
    expect(screen.queryByRole("img")).not.toBeInTheDocument();

    scrollNear(container);

    expect(await screen.findByRole("img", { name: "cat.jpg" })).toHaveAttribute(
      "src",
      "blob:thumbnail-aaa",
    );
    expect(await screen.findByRole("img", { name: "IMG_0001.heic" })).toHaveAttribute(
      "src",
      "blob:thumbnail-bbb",
    );
    expect(fetches()).toEqual([
      { sha256: "aaa", version: "thumbnail" },
      { sha256: "bbb", version: "thumbnail" },
    ]);
    expect(fetchAsset).not.toHaveBeenCalled();
  });

  it("shows its file name, and fetches nothing, while the server has made no Thumbnail", () => {
    const { container } = renderWithProviders(
      <MessageAttachments message={message([{ ...photo, thumbnail_mime_type: null }])} />,
    );
    scrollNear(container);

    expect(screen.getByRole("button", { name: "Open cat.jpg" })).toHaveTextContent("cat.jpg");
    expect(screen.queryByRole("img")).not.toBeInTheDocument();
    expect(fetchAssetObjectUrl).not.toHaveBeenCalled();
  });

  it("opens the viewer when pressed", async () => {
    const user = setupUser();
    const onAttachmentClick = vi.fn();
    renderWithProviders(
      <MessageAttachments message={message([photo])} onAttachmentClick={onAttachmentClick} />,
    );

    await user.click(screen.getByRole("button", { name: "Open cat.jpg" }));

    expect(onAttachmentClick).toHaveBeenCalledWith(photo);
  });
});

/**
 * Downloading always gives the original, the record of what was sent, never
 * the Preview (`docs/architecture/media.md`, rule 2).
 */
describe("downloading an attachment", () => {
  it("fetches the original of a photo that has a Preview and saves it under its own name", async () => {
    const user = setupUser();
    const original = new Blob(["heic bytes"]);
    vi.mocked(fetchAsset).mockResolvedValue(original);
    vi.mocked(saveFile).mockResolvedValue(true);
    renderWithProviders(<MessageAttachments message={message([heic])} />);

    await user.click(screen.getByRole("button", { name: "Download IMG_0001.heic" }));

    await waitFor(() => expect(saveFile).toHaveBeenCalledWith("IMG_0001.heic", original));
    expect(fetchAsset).toHaveBeenCalledWith("bbb", { version: "original" });
  });

  it("is offered for a file the conversation cannot show", async () => {
    const user = setupUser();
    vi.mocked(fetchAsset).mockResolvedValue(new Blob(["pdf"]));
    renderWithProviders(
      <MessageAttachments
        message={message([
          attachment({ original_name: "lease.pdf", mime_type: "application/pdf", sha256: "fff" }),
        ])}
      />,
    );

    await user.click(screen.getByRole("button", { name: "Download lease.pdf" }));

    await waitFor(() => expect(fetchAsset).toHaveBeenCalledWith("fff", { version: "original" }));
  });

  it("says so when the download fails", async () => {
    const user = setupUser();
    vi.mocked(fetchAsset).mockRejectedValue(new Error("404"));
    renderWithProviders(
      <MessageAttachments
        message={message([
          attachment({ original_name: "lease.pdf", mime_type: "application/pdf", sha256: "fff" }),
        ])}
      />,
    );

    await user.click(screen.getByRole("button", { name: "Download lease.pdf" }));

    expect(await screen.findByText("Download failed")).toBeInTheDocument();
    expect(saveFile).not.toHaveBeenCalled();
  });
});

/**
 * Most videos in a conversation are scrolled past and never played, so a
 * video shows its Thumbnail and loads nothing of itself until play is
 * pressed. Then it streams through a Media Link, the Preview or the original
 * as its type decides (`docs/architecture/media.md`, rules 1, 2 and 5).
 */
describe("a video in the conversation", () => {
  const mov = attachment({
    original_name: "clip.mov",
    mime_type: "video/quicktime",
    sha256: "ccc",
    preview_mime_type: "video/mp4",
    thumbnail_mime_type: "image/jpeg",
  });

  it("shows its Thumbnail and a play button, and loads nothing of the video until play", async () => {
    const user = setupUser();
    const { container } = renderWithProviders(<MessageAttachments message={message([mov])} />);
    scrollNear(container);

    expect(await screen.findByRole("img", { name: "clip.mov" })).toHaveAttribute(
      "src",
      "blob:thumbnail-ccc",
    );
    expect(container.querySelector("video")).toBeNull();
    expect(createMediaLink).not.toHaveBeenCalled();
    expect(fetches()).toEqual([{ sha256: "ccc", version: "thumbnail" }]);

    await user.click(screen.getByRole("button", { name: "Play clip.mov" }));

    const video = await screen.findByLabelText("clip.mov", { selector: "video" });
    expect(createMediaLink).toHaveBeenCalledWith("ccc");
    expect(video).toHaveAttribute("src", LINK.preview_url);
    // Streamed by the element itself, never downloaded whole first.
    expect(fetchAsset).not.toHaveBeenCalled();
    expect(fetches()).toEqual([{ sha256: "ccc", version: "thumbnail" }]);
  });

  it("streams the original of an MP4, a type every browser plays", async () => {
    const user = setupUser();
    renderWithProviders(
      <MessageAttachments
        message={message([
          attachment({ original_name: "clip.mp4", mime_type: "video/mp4", sha256: "ddd" }),
        ])}
      />,
    );

    await user.click(screen.getByRole("button", { name: "Play clip.mp4" }));

    const video = await screen.findByLabelText("clip.mp4", { selector: "video" });
    expect(video).toHaveAttribute("src", LINK.url);
  });

  /** Fail the video element as the browser does, with `code` on its `error`. */
  function failVideo(video: HTMLElement, code: number) {
    Object.defineProperty(video, "error", { configurable: true, value: { code } });
    fireEvent.error(video);
  }

  it("goes back to its play button when the stream breaks off, and plays again through a new Media Link", async () => {
    // A Media Link ends after an hour or with the Session, and the element's
    // next range then fails as a network error. A player left holding the
    // dead link could not be started again without a reload.
    const user = setupUser();
    renderWithProviders(<MessageAttachments message={message([mov])} />);
    await user.click(screen.getByRole("button", { name: "Play clip.mov" }));
    failVideo(await screen.findByLabelText("clip.mov", { selector: "video" }), 2);

    expect(screen.getByText(STOPPED)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Play clip.mov" }));

    expect(await screen.findByLabelText("clip.mov", { selector: "video" })).toBeInTheDocument();
    expect(createMediaLink).toHaveBeenCalledTimes(2);
  });

  it("says the browser cannot play a file it cannot decode, and offers no play button to fail again", async () => {
    const user = setupUser();
    renderWithProviders(
      <MessageAttachments
        message={message([
          attachment({ original_name: "hevc.mp4", mime_type: "video/mp4", sha256: "ddd" }),
        ])}
      />,
    );
    await user.click(screen.getByRole("button", { name: "Play hevc.mp4" }));
    failVideo(await screen.findByLabelText("hevc.mp4", { selector: "video" }), 4);

    expect(screen.getByText(CANNOT_PLAY_HERE)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Play hevc.mp4" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Download hevc.mp4" })).toBeInTheDocument();
  });

  it("says a HEVC video with no Preview yet cannot be played, and offers the download", () => {
    renderWithProviders(
      <MessageAttachments message={message([{ ...mov, preview_mime_type: null }])} />,
    );

    expect(screen.getByText(NO_PLAYABLE_COPY)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Play clip.mov" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Download clip.mov" })).toBeInTheDocument();
  });

  it("shows a file chip when no file was stored, as a photo with no digest does", () => {
    renderWithProviders(
      <MessageAttachments
        message={message([
          attachment({ original_name: "clip.mov", mime_type: "video/quicktime", sha256: null }),
        ])}
      />,
    );
    expect(screen.getByText("clip.mov")).toBeInTheDocument();
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
  });
});

/** A voice note plays in the conversation, streamed only once play is pressed. */
describe("a recording in the conversation", () => {
  const amr = attachment({
    original_name: "voice.amr",
    mime_type: "audio/amr",
    sha256: "eee",
    preview_mime_type: "audio/mpeg",
  });

  it("loads nothing until play, then streams the Preview of an AMR voice note", async () => {
    const user = setupUser();
    const { container } = renderWithProviders(<MessageAttachments message={message([amr])} />);
    scrollNear(container);
    expect(container.querySelector("audio")).toBeNull();
    expect(createMediaLink).not.toHaveBeenCalled();

    await user.click(screen.getByRole("button", { name: "Play voice.amr" }));

    const audio = await screen.findByLabelText("voice.amr", { selector: "audio" });
    expect(createMediaLink).toHaveBeenCalledWith("eee");
    expect(audio).toHaveAttribute("src", LINK.preview_url);
    expect(fetchAssetObjectUrl).not.toHaveBeenCalled();
  });

  it("says an AMR voice note with no Preview yet cannot be played, and offers the download", () => {
    renderWithProviders(
      <MessageAttachments message={message([{ ...amr, preview_mime_type: null }])} />,
    );

    expect(screen.getByText(NO_PLAYABLE_COPY)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Play voice.amr" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Download voice.amr" })).toBeInTheDocument();
  });
});
