/** @vitest-environment jsdom */

import { fireEvent, render } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { setupUser } from "../test/user";
import ColumnResizeHandle from "./ColumnResizeHandle";
import ListColumn from "./ListColumn";

function handleProps() {
  return {
    onPointerDown: vi.fn(),
    onPointerMove: vi.fn(),
    onPointerUp: vi.fn(),
    onPointerCancel: vi.fn(),
    onKeyDown: vi.fn(),
    onMouseEnter: vi.fn(),
    onMouseLeave: vi.fn(),
  };
}

function renderHandle(props: ReturnType<typeof handleProps>) {
  return render(
    <ColumnResizeHandle
      ariaLabel="Resize navigation panel"
      width={220}
      minWidth={160}
      maxWidth={520}
      dragging={false}
      handleHover={false}
      handleProps={props}
    />,
  );
}

let props: ReturnType<typeof handleProps>;

beforeEach(() => {
  props = handleProps();
});

describe("ColumnResizeHandle", () => {
  it("is a native separator element, so it carries no role attribute of its own", () => {
    const { getByRole } = renderHandle(props);

    const handle = getByRole("separator", { name: "Resize navigation panel" });
    expect(handle.tagName).toBe("HR");
    expect(handle).not.toHaveAttribute("role");
    expect(handle).toHaveAttribute("tabindex", "0");
  });

  it("keeps the grip on the inner right edge so the next column cannot cover it", () => {
    const { getByRole } = renderHandle(props);

    const handle = getByRole("separator", { name: "Resize navigation panel" });
    expect(handle.className).toContain("right-0");
    expect(handle.className).not.toContain("translate-x-full");
  });

  /**
   * The seven handlers `useColumnResize` supplies are the whole point of the
   * component: it renders a strip and forwards them. None was ever invoked, so
   * a version that spread `handleProps` onto the decorative inner div — or
   * dropped the spread entirely — rendered the same grip and passed. Dragging
   * a panel would then do nothing at all.
   */
  it("forwards every pointer handler to the grip a person actually drags", () => {
    const { getByRole } = renderHandle(props);
    const handle = getByRole("separator", { name: "Resize navigation panel" });

    fireEvent.pointerDown(handle);
    expect(props.onPointerDown).toHaveBeenCalledTimes(1);

    fireEvent.pointerMove(handle);
    expect(props.onPointerMove).toHaveBeenCalledTimes(1);

    fireEvent.pointerUp(handle);
    expect(props.onPointerUp).toHaveBeenCalledTimes(1);

    fireEvent.pointerCancel(handle);
    expect(props.onPointerCancel).toHaveBeenCalledTimes(1);
  });

  it("forwards the hover handlers, which is what draws the accent line", async () => {
    const user = setupUser();
    const { getByRole } = renderHandle(props);
    const handle = getByRole("separator", { name: "Resize navigation panel" });

    await user.hover(handle);
    expect(props.onMouseEnter).toHaveBeenCalledTimes(1);

    await user.unhover(handle);
    expect(props.onMouseLeave).toHaveBeenCalledTimes(1);
  });

  it("takes focus and forwards keys, so the column can be resized without a mouse", async () => {
    const user = setupUser();
    const { getByRole } = renderHandle(props);
    const handle = getByRole("separator", { name: "Resize navigation panel" });

    await user.tab();
    expect(handle).toHaveFocus();

    await user.keyboard("{ArrowRight}");
    expect(props.onKeyDown).toHaveBeenCalled();
    expect(props.onKeyDown.mock.calls.at(-1)?.[0]).toMatchObject({ key: "ArrowRight" });
  });

  it("reports the width it is at and the range it may move in, for a screen reader", () => {
    const { getByRole } = renderHandle(props);
    const handle = getByRole("separator", { name: "Resize navigation panel" });

    expect(handle).toHaveAttribute("aria-valuenow", "220");
    expect(handle).toHaveAttribute("aria-valuemin", "160");
    expect(handle).toHaveAttribute("aria-valuemax", "520");
    expect(handle).toHaveAttribute("aria-orientation", "vertical");
  });

  it("reports the width on screen when the window squeezes the column below its stored width", () => {
    const column = document.createElement("div");
    document.body.appendChild(column);
    vi.spyOn(column, "getBoundingClientRect").mockReturnValue({
      width: 250,
      height: 100,
      top: 0,
      left: 0,
      bottom: 100,
      right: 250,
      x: 0,
      y: 0,
      toJSON: () => ({}),
    });
    const { getByRole } = render(
      <ColumnResizeHandle
        ariaLabel="Resize list"
        width={400}
        minWidth={160}
        maxWidth={520}
        dragging={false}
        handleHover={false}
        handleProps={props}
      />,
      { container: column },
    );

    expect(getByRole("separator", { name: "Resize list" })).toHaveAttribute("aria-valuenow", "250");
    column.remove();
  });
});

/**
 * The list column with its real resize hook. jsdom lays nothing out, so the
 * column reports no painted width and the grip falls back to the stored width,
 * which is what `aria-valuenow` and the column's own style then show.
 */
describe("ColumnResizeHandle in a resizable column", () => {
  const capture = new Set<number>();

  beforeEach(() => {
    capture.clear();
    Element.prototype.setPointerCapture = (id: number) => {
      capture.add(id);
    };
    Element.prototype.hasPointerCapture = (id: number) => capture.has(id);
    Element.prototype.releasePointerCapture = (id: number) => {
      capture.delete(id);
    };
  });

  afterEach(() => {
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
