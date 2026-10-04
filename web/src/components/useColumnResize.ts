import type { KeyboardEvent, PointerEvent as ReactPointerEvent } from "react";
import { useEffect, useRef, useState } from "react";
import { clampWidth, loadWidth, saveWidth } from "./columnResize";

export type ColumnResizeHandleProps = {
  onPointerDown: (e: ReactPointerEvent<HTMLElement>) => void;
  onPointerMove: (e: ReactPointerEvent<HTMLElement>) => void;
  onPointerUp: (e: ReactPointerEvent<HTMLElement>) => void;
  onPointerCancel: (e: ReactPointerEvent<HTMLElement>) => void;
  onKeyDown: (e: KeyboardEvent<HTMLElement>) => void;
  onMouseEnter: () => void;
  onMouseLeave: () => void;
};

export type UseColumnResizeOptions = {
  storageKey: string;
  defaultWidth: number;
  minWidth: number;
  maxWidth: number;
  /**
   * The widest the column may be in this window, when that is less than
   * `maxWidth`. The returned `width`, a drag and a key all stop there, while a
   * stored width wider than it is kept for a wider window.
   */
  windowMaxWidth?: number;
  /** Called when a drag starts (true) or ends (false). */
  onDraggingChange?: (dragging: boolean) => void;
};

export type UseColumnResizeResult = {
  width: number;
  dragging: boolean;
  handleHover: boolean;
  handleProps: ColumnResizeHandleProps;
};

/** Clear body styles set for the duration of a column-width drag. */
function clearBodyDragStyles(): void {
  document.body.style.userSelect = "";
  document.body.style.cursor = "";
}

/**
 * Width of the column that owns the resize handle, as its `width` style
 * counts it, so a narrow window that squeezes the column is seen. The left
 * panel and the list column are content-box with a 1px right border, which the
 * browser paints outside that width, so the border and any padding come off
 * the painted width rather than adding 1px to every resize.
 * Falls back to preferredWidth when the parent is missing (tests / detached nodes).
 */
export function measureColumnWidth(handle: HTMLElement, preferredWidth: number): number {
  const parent = handle.parentElement;
  if (!parent) return preferredWidth;
  const painted = parent.getBoundingClientRect().width;
  if (!Number.isFinite(painted) || painted <= 0) return preferredWidth;
  const style = getComputedStyle(parent);
  if (style.boxSizing === "border-box") return painted;
  const px = (value: string) => Number.parseFloat(value) || 0;
  const outside =
    px(style.borderLeftWidth) +
    px(style.borderRightWidth) +
    px(style.paddingLeft) +
    px(style.paddingRight);
  return painted - outside;
}

/** Drag and keyboard resize for a vertical column, with localStorage persistence. */
export function useColumnResize({
  storageKey,
  defaultWidth,
  minWidth,
  maxWidth,
  windowMaxWidth,
  onDraggingChange,
}: UseColumnResizeOptions): UseColumnResizeResult {
  const [width, setWidth] = useState(() => loadWidth(storageKey, defaultWidth, minWidth, maxWidth));
  const effectiveMaxWidth = Math.min(maxWidth, windowMaxWidth ?? maxWidth);
  const [dragging, setDragging] = useState(false);
  const [handleHover, setHandleHover] = useState(false);

  const startXRef = useRef(0);
  const startWidthRef = useRef(defaultWidth);
  const widthRef = useRef(width);
  widthRef.current = width;
  const draggingRef = useRef(false);
  const onDraggingChangeRef = useRef(onDraggingChange);
  onDraggingChangeRef.current = onDraggingChange;

  const setDraggingState = (next: boolean) => {
    draggingRef.current = next;
    setDragging(next);
    onDraggingChangeRef.current?.(next);
  };

  // If the column unmounts mid-drag (route change), clear body styles and context.
  useEffect(() => {
    return () => {
      if (!draggingRef.current) return;
      draggingRef.current = false;
      clearBodyDragStyles();
      onDraggingChangeRef.current?.(false);
    };
  }, []);

  const endDrag = (el: HTMLElement, pointerId: number) => {
    if (el.hasPointerCapture(pointerId)) {
      el.releasePointerCapture(pointerId);
    }
    setDraggingState(false);
    clearBodyDragStyles();
    saveWidth(storageKey, widthRef.current);
  };

  const onResizePointerDown = (e: ReactPointerEvent<HTMLElement>) => {
    e.preventDefault();
    e.currentTarget.setPointerCapture(e.pointerId);
    startXRef.current = e.clientX;
    // Start from the column's measured width (`measureColumnWidth`), so a
    // flex-shrunk column does not jump to its preferred width.
    startWidthRef.current = measureColumnWidth(e.currentTarget, widthRef.current);
    setDraggingState(true);
    document.body.style.userSelect = "none";
    document.body.style.cursor = "col-resize";
  };

  const onResizePointerMove = (e: ReactPointerEvent<HTMLElement>) => {
    if (!e.currentTarget.hasPointerCapture(e.pointerId)) return;
    const next = clampWidth(
      startWidthRef.current + (e.clientX - startXRef.current),
      minWidth,
      effectiveMaxWidth,
    );
    widthRef.current = next;
    setWidth(next);
  };

  const onResizePointerUp = (e: ReactPointerEvent<HTMLElement>) => {
    endDrag(e.currentTarget, e.pointerId);
  };

  const onResizeKeyDown = (e: KeyboardEvent<HTMLElement>) => {
    const step = e.shiftKey ? 24 : 8;
    if (e.key === "ArrowLeft" || e.key === "ArrowRight") {
      e.preventDefault();
      // Start from the column's measured width, as a drag does, so a
      // flex-shrunk column moves on the first key press.
      const from = measureColumnWidth(e.currentTarget, widthRef.current);
      const next = clampWidth(
        e.key === "ArrowLeft" ? from - step : from + step,
        minWidth,
        effectiveMaxWidth,
      );
      widthRef.current = next;
      setWidth(next);
      saveWidth(storageKey, next);
    } else if (e.key === "Home") {
      e.preventDefault();
      widthRef.current = minWidth;
      setWidth(minWidth);
      saveWidth(storageKey, minWidth);
    } else if (e.key === "End") {
      e.preventDefault();
      widthRef.current = effectiveMaxWidth;
      setWidth(effectiveMaxWidth);
      saveWidth(storageKey, effectiveMaxWidth);
    }
  };

  return {
    width: Math.min(width, effectiveMaxWidth),
    dragging,
    handleHover,
    handleProps: {
      onPointerDown: onResizePointerDown,
      onPointerMove: onResizePointerMove,
      onPointerUp: onResizePointerUp,
      onPointerCancel: onResizePointerUp,
      onKeyDown: onResizeKeyDown,
      onMouseEnter: () => setHandleHover(true),
      onMouseLeave: () => setHandleHover(false),
    },
  };
}
