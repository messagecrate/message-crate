import type { ReactNode } from "react";

/**
 * A photo or video in the conversation: a frame of one height, so a
 * Thumbnail that loads above the messages on screen does not push them down,
 * holding whatever the caller draws over it.
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
    return <img src={url} alt={name} className="block h-full w-auto max-w-[278px] object-cover" />;
  }
  return (
    <span className="flex h-full w-[200px] items-center justify-center break-all bg-elevated px-3 text-center text-[0.75rem] text-muted">
      {name}
    </span>
  );
}
