import type { ReactNode } from "react";
import type { Thumbnail } from "../hooks/useThumbnail";

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

/**
 * The Thumbnail, or the file's name in its place while there is none, with
 * the Thumbnail's `note` under it when the server says why it could not make
 * one. The name gives up its room to the note, so a long name is cut short
 * rather than drawn over the note.
 */
export function ThumbnailPicture({ name, thumbnail }: { name: string; thumbnail: Thumbnail }) {
  const { url, note } = thumbnail;
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
    <span className="flex h-full w-[200px] flex-col bg-elevated px-3 text-center text-[0.75rem] text-muted">
      <span className="flex min-h-0 flex-1 items-center justify-center overflow-hidden break-all">
        {name}
      </span>
      {note ? (
        <span title={note} className="mb-2 line-clamp-2 shrink-0 break-words text-[0.688rem]">
          {note}
        </span>
      ) : null}
    </span>
  );
}
