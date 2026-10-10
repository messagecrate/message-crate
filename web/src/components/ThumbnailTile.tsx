import type { ReactNode } from "react";

/**
 * A photo or video in the conversation: a frame of one height, so a
 * Thumbnail that loads above the messages on screen does not push them down,
 * holding whatever the caller draws over it.
 *
 * The frame's 280 px cap is in pixels, because the message around it sizes
 * itself to its content and a percentage cap is ignored while it does; with
 * one the message grew to the picture's own width. The picture's cap is a
 * percentage as well, which also makes it shrink below its own width, so in
 * a message narrower than 280 px, as in the right pane's 320 px minimum
 * (#1722), the frame takes the message's width and object-cover crops the
 * picture instead of the conversation scrolling sideways.
 */
export function ThumbnailTile({
  tileRef,
  children,
}: {
  tileRef: (node: HTMLDivElement | null) => void;
  children: ReactNode;
}) {
  return (
    <div ref={tileRef} className="relative mt-1.5 h-[200px] w-fit max-w-[280px]">
      {children}
    </div>
  );
}

/** The Thumbnail, or the file's name in its place while there is none. */
export function ThumbnailPicture({ name, url }: { name: string; url: string | null }) {
  if (url) {
    return (
      <img
        src={url}
        alt={name}
        className="block h-full w-auto max-w-[min(278px,100%)] object-cover"
      />
    );
  }
  return (
    <span className="flex h-full w-[200px] items-center justify-center break-all bg-elevated px-3 text-center text-[0.75rem] text-muted">
      {name}
    </span>
  );
}
