import { useServerInfo } from "../../lib/useServerInfo";

/**
 * Says so on every Owner Home section when the database runs without
 * write-ahead logging. The server asks for it on every start and goes on
 * without it when SQLite says no; then a reader waits for a writer, so every
 * page load waits while an import batch writes. Before this line, the only
 * sign was a warning in the server's log (#2201). A mode the server could not
 * read (`null`) says nothing either way, so it shows nothing.
 */
export function JournalModeNotice() {
  const { data } = useServerInfo();
  if (!data?.journal_mode || data.journal_mode === "wal") return null;

  return (
    <p
      role="status"
      className="m-0 shrink-0 border-b border-warn-soft-border bg-warn-soft-bg px-3 py-1.5 text-[0.813rem] text-warn-soft-text"
    >
      The server could not turn on write-ahead logging when it started, so every page waits while an
      import is writing. The server's log, under Logs, names the cause. If another program has the
      database file open, close it. Then restart the server, which also clears a journal left behind
      by a server that stopped part-way.
    </p>
  );
}
