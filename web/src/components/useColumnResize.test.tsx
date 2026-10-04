/** @vitest-environment jsdom */

import { act, renderHook } from "@testing-library/react";
import type { KeyboardEvent as ReactKeyboardEvent, PointerEvent as ReactPointerEvent } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { measureColumnWidth, useColumnResize } from "./useColumnResize";

afterEach(() => {
  localStorage.clear();
  document.body.style.cursor = "";
  document.body.style.userSelect = "";
});

describe("measureColumnWidth", () => {
  it("reads the parent column width so a flex-shrunk drag starts from the screen", () => {
    const parent = document.createElement("div");
    const handle = document.createElement("div");
    parent.appendChild(handle);
    vi.spyOn(parent, "getBoundingClientRect").mockReturnValue({
      width: 80,
      height: 100,
      top: 0,
      left: 0,
      bottom: 100,
      right: 80,
      x: 0,
      y: 0,
      toJSON: () => ({}),
    });
    expect(measureColumnWidth(handle, 300)).toBe(80);
  });

  it("leaves out a content-box column's border, which its width does not include", () => {
    expect(measureColumnWidth(handleInColumn(221, BORDERED_COLUMN), 300)).toBe(220);
  });

  it("keeps a border-box column's border, which its width includes", () => {
    const style = "box-sizing: border-box; border-right: 1px solid; padding: 0 4px";
    expect(measureColumnWidth(handleInColumn(221, style), 300)).toBe(221);
  });

  it("leaves out a content-box column's padding as well", () => {
    const style = "box-sizing: content-box; border-right: 1px solid; padding: 0 4px";
    expect(measureColumnWidth(handleInColumn(229, style), 300)).toBe(220);
  });

  it("falls back to preferred width when detached", () => {
    const handle = document.createElement("div");
    expect(measureColumnWidth(handle, 300)).toBe(300);
  });
});

/**
 * A handle inside a column the browser has painted at `painted` px wide.
 * `style` is the column's own CSS, such as the 1px right border the left panel
 * and the list column draw outside their width.
 */
function handleInColumn(painted: number, style = ""): HTMLDivElement {
  const parent = document.createElement("div");
  parent.setAttribute("style", style);
  const handle = document.createElement("div");
  parent.appendChild(handle);
  vi.spyOn(parent, "getBoundingClientRect").mockReturnValue({
    width: painted,
    height: 100,
    top: 0,
    left: 0,
    bottom: 100,
    right: painted,
    x: 0,
    y: 0,
    toJSON: () => ({}),
  });
  return handle;
}

/** The CSS of the left panel and the list column that changes their painted width. */
const BORDERED_COLUMN = "box-sizing: content-box; border-right: 1px solid";

describe("useColumnResize", () => {
  it("narrows a squeezed column from its painted width on one arrow key", () => {
    const { result } = renderHook(() =>
      useColumnResize({
        storageKey: "testCol:v1",
        defaultWidth: 400,
        minWidth: 160,
        maxWidth: 520,
      }),
    );

    act(() => {
      result.current.handleProps.onKeyDown({
        key: "ArrowLeft",
        shiftKey: false,
        preventDefault: () => {},
        currentTarget: handleInColumn(250),
      } as unknown as ReactKeyboardEvent<HTMLDivElement>);
    });

    expect(result.current.width).toBe(242);
  });

  /**
   * The left panel and the list column are content-box with a 1px right
   * border, so the browser paints them 1px wider than their width. Starting
   * from the painted width made each ArrowRight move 9px and each ArrowLeft 7px.
   */
  it.each([
    { keys: "ArrowRight", shiftKey: false, expected: 228 },
    { keys: "ArrowLeft", shiftKey: false, expected: 212 },
    { keys: "ArrowRight", shiftKey: true, expected: 244 },
    { keys: "ArrowLeft", shiftKey: true, expected: 196 },
  ])(
    "moves a bordered column by exactly the step on $keys (Shift: $shiftKey)",
    ({ keys, shiftKey, expected }) => {
      const { result } = renderHook(() =>
        useColumnResize({
          storageKey: "testCol:v1",
          defaultWidth: 220,
          minWidth: 160,
          maxWidth: 360,
        }),
      );

      act(() => {
        result.current.handleProps.onKeyDown({
          key: keys,
          shiftKey,
          preventDefault: () => {},
          currentTarget: handleInColumn(221, BORDERED_COLUMN),
        } as unknown as ReactKeyboardEvent<HTMLDivElement>);
      });

      expect(result.current.width).toBe(expected);
    },
  );

  it("leaves a bordered column's width as it was after a drag that does not move", () => {
    const { result } = renderHook(() =>
      useColumnResize({
        storageKey: "testCol:v1",
        defaultWidth: 220,
        minWidth: 160,
        maxWidth: 360,
      }),
    );
    const handle = handleInColumn(221, BORDERED_COLUMN);
    let captured = false;
    handle.setPointerCapture = () => {
      captured = true;
    };
    handle.hasPointerCapture = () => captured;
    handle.releasePointerCapture = () => {
      captured = false;
    };
    const pointer = (clientX: number) =>
      ({
        preventDefault: () => {},
        pointerId: 1,
        clientX,
        currentTarget: handle,
      }) as unknown as ReactPointerEvent<HTMLDivElement>;

    act(() => result.current.handleProps.onPointerDown(pointer(100)));
    act(() => result.current.handleProps.onPointerMove(pointer(100)));
    act(() => result.current.handleProps.onPointerUp(pointer(100)));

    expect(result.current.width).toBe(220);
    expect(localStorage.getItem("testCol:v1")).toBe("220");
  });

  it("clears body drag styles and reports false when unmounted mid-drag", () => {
    const onDraggingChange = vi.fn();
    const { result, unmount } = renderHook(() =>
      useColumnResize({
        storageKey: "testCol:v1",
        defaultWidth: 220,
        minWidth: 160,
        maxWidth: 360,
        onDraggingChange,
      }),
    );

    result.current.handleProps.onPointerDown({
      preventDefault: () => {},
      pointerId: 1,
      clientX: 100,
      currentTarget: {
        setPointerCapture: () => {},
        parentElement: null,
      },
    } as unknown as ReactPointerEvent<HTMLDivElement>);

    expect(onDraggingChange).toHaveBeenCalledWith(true);
    expect(document.body.style.cursor).toBe("col-resize");
    expect(document.body.style.userSelect).toBe("none");

    unmount();

    expect(onDraggingChange).toHaveBeenCalledWith(false);
    expect(document.body.style.cursor).toBe("");
    expect(document.body.style.userSelect).toBe("");
  });
});
