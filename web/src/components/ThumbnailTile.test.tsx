/** @vitest-environment jsdom */

import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { ThumbnailPicture, ThumbnailTile } from "./ThumbnailTile";

afterEach(() => {
  cleanup();
});

describe("ThumbnailTile", () => {
  // jsdom lays nothing out, so the test holds the classes. In a 320 px right
  // pane a message is 225 px wide, and a picture with a pixel cap made the
  // thread scroll sideways (#1722). A frame with a percentage cap let the
  // message, which sizes itself to its content, grow to the picture's own
  // width, and the frame sat at its left edge rather than flush right.
  it("is never wider than the message it sits in, nor wider than 280 px", () => {
    render(
      <ThumbnailTile tileRef={() => {}}>
        <ThumbnailPicture name="photo.jpg" url="blob:photo" />
      </ThumbnailTile>,
    );
    const picture = screen.getByRole("img", { name: "photo.jpg" });
    expect(picture.className).toContain("max-w-[min(278px,100%)]");
    expect(picture.parentElement?.className.split(" ")).toContain("max-w-[280px]");
  });
});
