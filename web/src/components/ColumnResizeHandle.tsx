import { useLayoutEffect, useRef, useState } from "react";
import { Z_RESIZE_HANDLE } from "../lib/zLayers";
import { type ColumnResizeHandleProps, measureColumnWidth } from "./useColumnResize";

/**
 * Vertical grip on the right edge of a resizable panel (the left panel and the
 * list column). It is a native `<hr>`, whose role is separator, rather than
 * React Aria's `ColumnResizer`: that one resizes the columns of a React Aria
 * `Table`, and these are layout panels, not table columns (#1553).
 */
export default function ColumnResizeHandle({
  ariaLabel,
  width,
  minWidth,
  maxWidth,
  dragging,
  handleHover,
  handleProps,
}: {
  ariaLabel: string;
  width: number;
  minWidth: number;
  maxWidth: number;
  dragging: boolean;
  handleHover: boolean;
  handleProps: ColumnResizeHandleProps;
}) {
  const gripRef = useRef<HTMLHRElement>(null);
  // The width on screen, which a narrow window can squeeze below `width`.
  const [painted, setPainted] = useState(width);

  useLayoutEffect(() => {
    const grip = gripRef.current;
    if (!grip) return;
    const measure = () => setPainted(Math.round(measureColumnWidth(grip, width)));
    measure();
    const column = grip.parentElement;
    if (!column) return;
    const observer = new ResizeObserver(measure);
    observer.observe(column);
    return () => observer.disconnect();
  }, [width]);

  return (
    <hr
      ref={gripRef}
      aria-orientation="vertical"
      aria-label={ariaLabel}
      aria-valuenow={painted}
      aria-valuemin={minWidth}
      aria-valuemax={maxWidth}
      tabIndex={0}
      data-active={dragging || handleHover || undefined}
      {...handleProps}
      // w-2 matches `resizeHandleGutter`, the inset each resizable panel puts on its
      // scrolling child so this strip does not cover the scrollbar. The accent line
      // is the strip's ::after, drawn on its right edge while it is hovered or dragged.
      className={`absolute top-0 right-0 m-0 h-full w-2 touch-none cursor-col-resize border-0 bg-transparent outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-accent after:pointer-events-none after:absolute after:top-0 after:right-0 after:bottom-0 after:w-px after:bg-transparent after:content-[''] data-active:after:bg-accent ${Z_RESIZE_HANDLE}`}
    />
  );
}
