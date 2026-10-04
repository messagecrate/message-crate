import { useMutation } from "@tanstack/react-query";
import { type SyntheticEvent, useState } from "react";
import { fullVersion } from "../lib/attachmentMedia";
import { createMediaLink } from "../lib/serverApi";
import type { MessageAttachment } from "../lib/types";

/** `MediaError.MEDIA_ERR_NETWORK`: the stream broke off, rather than the file being unplayable. */
const MEDIA_ERR_NETWORK = 2;

/** What a player says about a file of a type browsers often cannot play, before its Preview is made. */
export const NO_PLAYABLE_COPY = "No copy a browser can play yet";

/** What a player says when its stream broke off, as it does when its Media Link ends. */
export const STOPPED = "Playback stopped. Press play to start again.";

/** What a player says when the browser could not play the file it was given. */
export const CANNOT_PLAY_HERE = "This browser cannot play the file. Download it to play it.";

/**
 * A video or recording that streams once play is pressed
 * (`docs/architecture/media.md`, rules 1, 2 and 5). `play` makes a Media
 * Link; `src` is then its URL for the original or the Preview, as the type
 * decides, for a `<video>` or `<audio>` that loads it a range at a time.
 *
 * A Media Link ends after an hour, or sooner when the Session ends, and the
 * element's next range then fails as a network error. `onMediaError` puts
 * the play button back for that, so pressing it makes a new link rather than
 * leaving a dead player. Any other error means the browser cannot play the
 * file itself, such as a HEVC MP4 whose Preview is not made yet, and a new
 * link would fail the same way, so the player says so and leaves the
 * download.
 */
export function useStreamedMedia(attachment: MessageAttachment) {
  const version = fullVersion(attachment);
  const link = useMutation({ mutationFn: () => createMediaLink(attachment.sha256 ?? "") });
  const [failure, setFailure] = useState<"stopped" | "unplayable" | null>(null);
  const src = link.data ? (version === "preview" ? link.data.preview_url : link.data.url) : null;
  return {
    playable: version !== "none" && failure !== "unplayable",
    src,
    pending: link.isPending,
    note:
      version === "none"
        ? NO_PLAYABLE_COPY
        : link.isError
          ? "Could not start playing"
          : failure === "stopped"
            ? STOPPED
            : failure === "unplayable"
              ? CANNOT_PLAY_HERE
              : null,
    play: () => {
      setFailure(null);
      link.mutate();
    },
    onMediaError: (event: SyntheticEvent<HTMLMediaElement>) => {
      const code = event.currentTarget.error?.code;
      setFailure(code === MEDIA_ERR_NETWORK ? "stopped" : "unplayable");
      link.reset();
    },
  };
}
