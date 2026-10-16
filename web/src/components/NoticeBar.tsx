import { useLocation } from "react-router-dom";
import { getBaseUrl } from "../lib/api";
import { isOwnAddress } from "../lib/localServer";
import Button from "./Button";
import { type ShownLocalServerNotice, useLocalServerNotice } from "./useLocalServerNotice";

/** What the bar says while the app starts its own server. */
function startingText(
  notice: Extract<ShownLocalServerNotice, { state: "restarting" | "starting" }>,
): string {
  if (notice.state === "starting") return "Starting the server…";
  return notice.cause === "unanswered"
    ? "The server stopped answering. Restarting it…"
    : "The server stopped. Restarting it…";
}

/**
 * The bar across the top of the page, above every screen, for what the
 * person needs to know whichever screen is open. Today it says when the
 * desktop app's own Message Crate stopped and is being started again
 * (#2695): the notice clears by itself once the server answers, and stays,
 * saying why, when it could not be started. Empty, it takes no space.
 *
 * It says nothing on the login card, which shows the server's state itself,
 * nor while the window uses a Message Crate at another address, which the
 * app's own server does not serve.
 */
export default function NoticeBar() {
  const { notice, retry } = useLocalServerNotice();
  const { pathname } = useLocation();
  if (notice === null || pathname === "/login" || !isOwnAddress(getBaseUrl())) return null;
  if (notice.state !== "failed") {
    return (
      <div
        role="status"
        className="shrink-0 border-warn-soft-border border-b bg-warn-soft-bg px-4 py-2 text-[0.813rem] text-warn-soft-text"
      >
        {startingText(notice)}
      </div>
    );
  }
  return (
    <div
      role="alert"
      className="flex shrink-0 items-center gap-3 border-danger-soft-border border-b bg-danger-soft-bg px-4 py-2 text-[0.813rem] text-danger"
    >
      <p className="m-0 min-w-0 flex-1">{notice.message}</p>
      <Button variant="secondary" size="xs" onPress={retry}>
        Try again
      </Button>
    </div>
  );
}
