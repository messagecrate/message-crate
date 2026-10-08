/** @vitest-environment jsdom */

import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { ThumbnailPicture, ThumbnailTile } from "./ThumbnailTile";

afterEach(() => {
  cleanup();
});

describe("ThumbnailTile", () => {
  // jsdom lays nothing out, so the test holds the classes: in a 320 px right
  // pane a message is 225 px wide, and a 280 px picture made the thread
  // scroll sideways (#1722).
  it("is never wider than the message it sits in", () => {
    render(
      <ThumbnailTile tileRef={() => {}}>
        <ThumbnailPicture name="photo.jpg" url="blob:photo" />
      </ThumbnailTile>,
    );
    const picture = screen.getByRole("img", { name: "photo.jpg" });
    expect(picture.className).toContain("max-w-[min(278px,100%)]");
    expect(picture.parentElement?.className).toContain("max-w-[min(280px,100%)]");
  });
});
