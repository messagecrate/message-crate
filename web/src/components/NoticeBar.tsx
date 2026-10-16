import Button from "./Button";
import { useLocalServerNotice } from "./useLocalServerNotice";

/**
 * The bar across the top of the page, above every screen, for what the
 * person needs to know whichever screen is open. Today it says when the
 * desktop app's own Message Crate stopped and is being started again
 * (#2695): the notice clears by itself once the server answers, and stays,
 * saying why, when it could not be started. Empty, it takes no space.
 */
export default function NoticeBar() {
  const { notice, retry } = useLocalServerNotice();
  if (notice === null) return null;
  if (notice.state === "restarting") {
    return (
      <div
        role="status"
        className="shrink-0 border-warn-soft-border border-b bg-warn-soft-bg px-4 py-2 text-[0.813rem] text-warn-soft-text"
      >
        The server stopped answering. Restarting it…
      </div>
    );
  }
  return (
    <div
      role="alert"
      className="flex shrink-0 items-center gap-3 border-danger-soft-border border-b bg-danger-soft-bg px-4 py-2 text-[0.813rem] text-danger"
    >
      <p className="m-0 min-w-0 flex-1">
        The server stopped answering and could not be started again. {notice.message}
      </p>
      <Button variant="secondary" size="xs" onPress={retry}>
        Try again
      </Button>
    </div>
  );
}
