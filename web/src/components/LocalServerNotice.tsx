import { getBaseUrl } from "../lib/api";
import { isOwnAddress } from "../lib/localServer";
import Button from "./Button";
import { type ShownLocalServerNotice, useLocalServerNotice } from "./useLocalServerNotice";

/** What the notice says while the app starts its own server. */
function startingText(
  notice: Extract<ShownLocalServerNotice, { state: "restarting" | "starting" }>,
): string {
  if (notice.state === "starting") return "Starting the server…";
  return notice.cause === "unanswered"
    ? "The server stopped answering. Restarting it…"
    : "The server stopped. Restarting it…";
}

/**
 * Says so across the top of the page, under the header beside the other
 * notices (#2991), when the desktop app's own Message Crate stopped and is
 * being started again (#2695). The notice clears by itself once the server
 * answers, and stays, saying why, with Try again, when it could not be
 * started. The login card has no header and shows the server's state
 * itself. Nothing shows while the window uses a Message Crate at another
 * address, which the app's own server does not serve.
 */
export default function LocalServerNotice() {
  const { notice, retry } = useLocalServerNotice();
  if (notice === null || !isOwnAddress(getBaseUrl())) return null;
  if (notice.state !== "failed") {
    return (
      <div
        role="status"
        className="shrink-0 border-warn-soft-border border-b bg-warn-soft-bg px-3 py-1.5 text-center text-[0.813rem] text-warn-soft-text"
      >
        {startingText(notice)}
      </div>
    );
  }
  return (
    <div
      role="alert"
      className="flex shrink-0 items-center justify-center gap-3 border-danger-soft-border border-b bg-danger-soft-bg px-3 py-1.5 text-[0.813rem] text-danger"
    >
      <p className="m-0">{notice.message}</p>
      <Button variant="secondary" size="chip" onPress={retry}>
        Try again
      </Button>
    </div>
  );
}
