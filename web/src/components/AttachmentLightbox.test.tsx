/** @vitest-environment jsdom */

import { act, fireEvent, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { saveFile } from "../lib/saveFile";
import { fetchAsset, fetchAssetObjectUrl } from "../lib/serverApi";
import type { MessageAttachment } from "../lib/types";
import { attachment } from "../test/apiShapes";
import { renderWithProviders } from "../test/providers";
import { setupUser } from "../test/user";
import AttachmentLightbox from "./AttachmentLightbox";

vi.mock("../lib/serverApi", () => ({
  fetchAsset: vi.fn(),
  fetchAssetObjectUrl: vi.fn(),
}));
vi.mock("../lib/saveFile", () => ({ saveFile: vi.fn() }));

const render = renderWithProviders;

const items: MessageAttachment[] = [
  attachment({
    path: "first.png",
    original_name: "first.png",
    mime_type: "image/png",
    sha256: "aaa",
    is_sticker: false,
    transcription: null,
  }),
  attachment({
    path: "second.png",
    original_name: "second.png",
    mime_type: "image/png",
    sha256: "bbb",
    is_sticker: false,
    transcription: null,
  }),
];

beforeEach(() => {
  vi.mocked(fetchAssetObjectUrl).mockImplementation(
    async (sha, options) => `blob:${options?.version ?? "original"}-${sha}`,
  );
});

/** Each fetch of attachment bytes: whose, and which version. */
function fetches(): { sha256: string; version: string | undefined }[] {
  return vi
    .mocked(fetchAssetObjectUrl)
    .mock.calls.map(([sha256, options]) => ({ sha256, version: options?.version }));
}

afterEach(() => {
  vi.clearAllMocks();
});

/** Render the viewer over `items`, with a spy on each callback. */
function open(currentIndex = 0, over: MessageAttachment[] = items) {
  const spies = { onClose: vi.fn(), onPrev: vi.fn(), onNext: vi.fn() };
  render(<AttachmentLightbox items={over} currentIndex={currentIndex} {...spies} />);
  return spies;
}

describe("AttachmentLightbox", () => {
  it("moves to the next attachment when ArrowRight is pressed", async () => {
    const onNext = vi.fn();
    const onPrev = vi.fn();
    render(
      <AttachmentLightbox
        items={items}
        currentIndex={0}
        onClose={() => {}}
        onPrev={onPrev}
        onNext={onNext}
      />,
    );
    await screen.findByRole("img", { name: "first.png" });

    fireEvent.keyDown(document.body, { key: "ArrowRight" });

    expect(onNext).toHaveBeenCalledTimes(1);
    expect(onPrev).not.toHaveBeenCalled();
  });

  it("moves to the previous attachment when ArrowLeft is pressed", async () => {
    const onPrev = vi.fn();
    const onNext = vi.fn();
    render(
      <AttachmentLightbox
        items={items}
        currentIndex={1}
        onClose={() => {}}
        onPrev={onPrev}
        onNext={onNext}
      />,
    );
    await screen.findByRole("img", { name: "second.png" });

    fireEvent.keyDown(document.body, { key: "ArrowLeft" });

    expect(onPrev).toHaveBeenCalledTimes(1);
    expect(onNext).not.toHaveBeenCalled();
  });
});

/**
 * The three buttons on the viewer.
 *
 * Only the arrow keys were tested, so the buttons a person actually clicks
 * were not: a viewer whose Next button called `onPrev`, or whose Close did
 * nothing, passed the file. These press each one.
 */
describe("AttachmentLightbox buttons", () => {
  it("moves forward and back with the on-screen arrows", async () => {
    const user = setupUser();
    const spies = open(0);
    await screen.findByRole("img", { name: "first.png" });

    await user.click(screen.getByRole("button", { name: "Next attachment" }));
    expect(spies.onNext).toHaveBeenCalledTimes(1);
    expect(spies.onPrev).not.toHaveBeenCalled();

    await user.click(screen.getByRole("button", { name: "Previous attachment" }));
    expect(spies.onPrev).toHaveBeenCalledTimes(1);
    expect(spies.onNext).toHaveBeenCalledTimes(1);
  });

  it("closes on the close button", async () => {
    const user = setupUser();
    const spies = open(0);
    await screen.findByRole("img", { name: "first.png" });

    await user.click(screen.getByRole("button", { name: "Close attachment viewer" }));

    expect(spies.onClose).toHaveBeenCalledTimes(1);
  });

  it("closes on Escape, which is the way out people try first", async () => {
    const spies = open(0);
    await screen.findByRole("img", { name: "first.png" });

    fireEvent.keyDown(document.activeElement ?? document.body, { key: "Escape" });

    expect(spies.onClose).toHaveBeenCalled();
  });

  it("hides the arrows when there is only one attachment", async () => {
    open(0, [items[0]]);
    await screen.findByRole("img", { name: "first.png" });

    expect(screen.queryByRole("button", { name: "Next attachment" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Previous attachment" })).not.toBeInTheDocument();
    // Close is always there: it is the only way out that does not need a key.
    expect(screen.getByRole("button", { name: "Close attachment viewer" })).toBeInTheDocument();
  });

  it("says which of how many is on screen", async () => {
    open(1);
    await screen.findByRole("img", { name: "second.png" });

    expect(screen.getByText("2 / 2")).toBeInTheDocument();
  });
});

/**
 * The viewer chooses the version by the photo's type before it fetches a
 * byte, never by a load that failed (`docs/architecture/media.md`, rule 2):
 * the original of a type every browser shows, the Preview of one browsers
 * often cannot, and neither while that Preview is not made yet.
 */
describe("AttachmentLightbox and the type rule", () => {
  const jpeg = attachment({
    original_name: "IMG_0002.jpg",
    mime_type: "image/jpeg",
    sha256: "jjj",
  });
  const heic = attachment({
    original_name: "IMG_0001.heic",
    mime_type: "image/heic",
    sha256: "ccc",
    preview_mime_type: "image/jpeg",
  });

  it("opens a JPEG's original", async () => {
    open(0, [jpeg]);

    expect(await screen.findByRole("img", { name: "IMG_0002.jpg" })).toHaveAttribute(
      "src",
      "blob:original-jjj",
    );
    expect(fetches()).toEqual([{ sha256: "jjj", version: "original" }]);
  });

  it("opens a HEIC's Preview, and never asks for the original", async () => {
    open(0, [heic]);

    expect(await screen.findByRole("img", { name: "IMG_0001.heic" })).toHaveAttribute(
      "src",
      "blob:preview-ccc",
    );
    expect(fetches()).toEqual([{ sha256: "ccc", version: "preview" }]);
  });

  it("says a HEIC with no Preview yet cannot be shown, and offers the original as a download", async () => {
    const user = setupUser();
    const original = new Blob(["heic"]);
    vi.mocked(fetchAsset).mockResolvedValue(original);
    open(0, [{ ...heic, preview_mime_type: null }]);

    expect(screen.getByText(/no copy a browser can show yet/)).toBeInTheDocument();
    expect(fetchAssetObjectUrl).not.toHaveBeenCalled();

    await user.click(screen.getAllByRole("button", { name: "Download IMG_0001.heic" })[0]);

    await waitFor(() => expect(saveFile).toHaveBeenCalledWith("IMG_0001.heic", original));
    expect(fetchAsset).toHaveBeenCalledWith("ccc", { version: "original" });
  });

  it("downloads the original of a photo it shows as its Preview", async () => {
    const user = setupUser();
    vi.mocked(fetchAsset).mockResolvedValue(new Blob(["heic"]));
    open(0, [heic]);
    await screen.findByRole("img", { name: "IMG_0001.heic" });

    await user.click(screen.getByRole("button", { name: "Download IMG_0001.heic" }));

    await waitFor(() => expect(fetchAsset).toHaveBeenCalledWith("ccc", { version: "original" }));
    expect(saveFile).toHaveBeenCalled();
  });
});

/**
 * The viewer never shows an empty frame, and stepping through a
 * conversation's photos does not wait on each one
 * (`docs/architecture/media.md`, rule 5).
 */
describe("AttachmentLightbox while it loads", () => {
  it("keeps the Thumbnail on screen until the full version is ready", async () => {
    let finish: (url: string) => void = () => {};
    vi.mocked(fetchAssetObjectUrl).mockImplementation((sha, options) =>
      options?.version === "thumbnail"
        ? Promise.resolve(`blob:thumbnail-${sha}`)
        : new Promise((resolve) => {
            finish = resolve;
          }),
    );
    open(0, [{ ...items[0], thumbnail_mime_type: "image/jpeg" }]);

    expect(await screen.findByRole("img", { name: "first.png" })).toHaveAttribute(
      "src",
      "blob:thumbnail-aaa",
    );
    expect(screen.getByText("Loading…")).toBeInTheDocument();

    await act(async () => finish("blob:original-aaa"));

    expect(screen.getByRole("img", { name: "first.png" })).toHaveAttribute(
      "src",
      "blob:original-aaa",
    );
    expect(screen.queryByText("Loading…")).not.toBeInTheDocument();
  });

  it("fetches the next and the previous photo along with the one on screen", async () => {
    const third: MessageAttachment = { ...items[0], original_name: "third.png", sha256: "ttt" };
    open(1, [...items, third]);
    await screen.findByRole("img", { name: "second.png" });

    expect(fetches()).toEqual(
      expect.arrayContaining([
        { sha256: "bbb", version: "original" },
        { sha256: "ttt", version: "original" },
        { sha256: "aaa", version: "original" },
      ]),
    );
    expect(fetches()).toHaveLength(3);
  });

  it("finds the next photo already loaded when it steps to it", async () => {
    const third: MessageAttachment = { ...items[0], original_name: "third.png", sha256: "ttt" };
    const fourth: MessageAttachment = { ...items[0], original_name: "fourth.png", sha256: "fff" };
    const all = [...items, third, fourth];
    const spies = { onClose: vi.fn(), onPrev: vi.fn(), onNext: vi.fn() };
    const { rerender } = render(<AttachmentLightbox items={all} currentIndex={0} {...spies} />);
    await screen.findByRole("img", { name: "first.png" });
    const before = fetches().length;

    rerender(<AttachmentLightbox items={all} currentIndex={1} {...spies} />);

    expect(screen.getByRole("img", { name: "second.png" })).toHaveAttribute(
      "src",
      "blob:original-bbb",
    );
    // Only the photo after the new one is fetched: the step itself fetched nothing.
    expect(fetches().slice(before)).toEqual([{ sha256: "ttt", version: "original" }]);
  });
});

describe("AttachmentLightbox while the file is not there", () => {
  it("says it is loading rather than showing a broken image", () => {
    vi.mocked(fetchAssetObjectUrl).mockReturnValue(new Promise(() => {}));
    open(0);

    expect(screen.getByText("Loading…")).toBeInTheDocument();
    expect(screen.queryByRole("img")).not.toBeInTheDocument();
  });

  it("says the attachment could not be loaded when the server refuses it", async () => {
    vi.mocked(fetchAssetObjectUrl).mockRejectedValue(new Error("404"));
    open(0);

    expect(await screen.findByText("Failed to load attachment")).toBeInTheDocument();
    expect(screen.queryByRole("img")).not.toBeInTheDocument();
  });

  it("renders nothing at all when the index names no attachment", () => {
    const spies = { onClose: vi.fn(), onPrev: vi.fn(), onNext: vi.fn() };
    const { container } = render(<AttachmentLightbox items={[]} currentIndex={0} {...spies} />);

    expect(container).toBeEmptyDOMElement();
  });
});
