/** @vitest-environment jsdom */

import { fireEvent, render } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { setupUser } from "../test/user";
import { ColumnResizeProvider } from "./ColumnResizeContext";
import ListColumn from "./ListColumn";

describe("ListColumn", () => {
  it("prefers the stored width but can shrink below it", () => {
    localStorage.setItem("listColumnWidth:v1", "300");
    const { container } = render(
      <ColumnResizeProvider>
        <ListColumn>
          <div>rows</div>
        </ListColumn>
      </ColumnResizeProvider>,
    );
    const column = container.querySelector("[data-list-column]");
    expect(column).toBeTruthy();
    expect(column).toHaveStyle({
      flex: "0 1 300px",
      minWidth: "0px",
      maxWidth: "300px",
      width: "300px",
    });
  });
});

/**
 * The list column with its real resize hook. jsdom lays nothing out, so the
 * column reports no painted width and the grip falls back to the stored width,
 * which is what `aria-valuenow` and the column's own style then show.
 */
describe("ListColumn's resize grip", () => {
  const capture = new Set<number>();
  // jsdom has no pointer capture; these stand in for it and are put back after each test.
  const proto = Element.prototype;
  const original = {
    set: proto.setPointerCapture,
    has: proto.hasPointerCapture,
    release: proto.releasePointerCapture,
  };

  beforeEach(() => {
    capture.clear();
    proto.setPointerCapture = (id: number) => {
      capture.add(id);
    };
    proto.hasPointerCapture = (id: number) => capture.has(id);
    proto.releasePointerCapture = (id: number) => {
      capture.delete(id);
    };
  });

  afterEach(() => {
    proto.setPointerCapture = original.set;
    proto.hasPointerCapture = original.has;
    proto.releasePointerCapture = original.release;
    localStorage.clear();
  });

  function renderColumn() {
    const view = render(
      <ListColumn>
        <p>Threads</p>
      </ListColumn>,
    );
    const handle = view.getByRole("separator", { name: "Resize list column" });
    const column = view.container.querySelector<HTMLElement>("[data-list-column]");
    if (!column) throw new Error("no list column");
    return { handle, column };
  }

  it("follows a drag and keeps the width the drag ended on", () => {
    const { handle, column } = renderColumn();
    expect(handle).toHaveAttribute("aria-valuenow", "300");

    fireEvent.pointerDown(handle, { pointerId: 1, clientX: 500 });
    fireEvent.pointerMove(handle, { pointerId: 1, clientX: 560 });
    expect(handle).toHaveAttribute("aria-valuenow", "360");
    expect(column.style.width).toBe("360px");

    fireEvent.pointerMove(handle, { pointerId: 1, clientX: 900 });
    expect(handle).toHaveAttribute("aria-valuenow", "560");

    fireEvent.pointerUp(handle, { pointerId: 1, clientX: 900 });
    fireEvent.pointerMove(handle, { pointerId: 1, clientX: 400 });
    expect(handle).toHaveAttribute("aria-valuenow", "560");
    expect(localStorage.getItem("listColumnWidth:v1")).toBe("560");
  });

  it("resizes from the keyboard with the arrow keys, Home and End", async () => {
    const user = setupUser();
    const { handle, column } = renderColumn();

    await user.tab();
    expect(handle).toHaveFocus();

    await user.keyboard("{ArrowRight}");
    expect(handle).toHaveAttribute("aria-valuenow", "308");
    await user.keyboard("{Shift>}{ArrowLeft}{/Shift}");
    expect(handle).toHaveAttribute("aria-valuenow", "284");
    expect(column.style.width).toBe("284px");

    await user.keyboard("{Home}");
    expect(handle).toHaveAttribute("aria-valuenow", "220");
    await user.keyboard("{End}");
    expect(handle).toHaveAttribute("aria-valuenow", "560");
    expect(handle).toHaveAttribute("aria-valuemin", "220");
    expect(handle).toHaveAttribute("aria-valuemax", "560");
  });
});
