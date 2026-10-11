import { useServerInfo } from "../../lib/useServerInfo";

/**
 * Says so on every Owner Home section when the database runs without
 * write-ahead logging. The server asks for it on every start and goes on
 * without it when SQLite says no; then a reader waits for a writer, so every
 * page load waits while an import batch writes. Before this line, the only
 * sign was a warning in the server's log (#2201).
 */
export function JournalModeNotice() {
  const { data } = useServerInfo();
  if (!data || data.journal_mode === "wal") return null;

  return (
    <p
      role="status"
      className="m-0 shrink-0 border-b border-warn-soft-border bg-warn-soft-bg px-3 py-1.5 text-[0.813rem] text-warn-soft-text"
    >
      The server could not turn on write-ahead logging when it started, so every page waits while an
      import is writing. Another program holding the database file, or a journal left behind by a
      server that stopped part-way, keeps it off. The server's log, under Logs, names the cause.
      Stop the other program, then restart the server.
    </p>
  );
}
