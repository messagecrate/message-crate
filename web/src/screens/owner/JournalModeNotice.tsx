import { useServerSettings } from "../../lib/useServerSettings";

/**
 * Says so on every Owner Home section when the database runs without
 * write-ahead logging. The server asks for it on every start and goes on
 * without it when SQLite says no; then a reader waits for a writer, so every
 * page load waits while an import batch writes. Before this line, the only
 * sign was a warning in the server's log (#2201). It quotes the error SQLite
 * gave when it gave one (#2691). A mode the server could not read (`null`)
 * says nothing either way, so it shows nothing.
 *
 * The mode is the owner's to read, on the server settings, not on the state
 * route anyone may ask (#2703).
 */
export function JournalModeNotice() {
  const { data } = useServerSettings();
  const journal = data?.journal;
  // SQLite's error says write-ahead logging was refused even when the mode
  // could not be read since.
  if (!journal || (!journal.wal_error && (!journal.mode || journal.mode === "wal"))) return null;

  return (
    <p
      role="status"
      className="m-0 shrink-0 border-b border-warn-soft-border bg-warn-soft-bg px-3 py-1.5 text-[0.813rem] text-warn-soft-text"
    >
      The server could not turn on write-ahead logging when it started, so every page waits while an
      import is writing.{" "}
      {journal.wal_error ? (
        <>
          SQLite answered: <span className="font-mono">{journal.wal_error}</span>.
        </>
      ) : (
        <>SQLite kept the database in journal mode {journal.mode} without giving an error.</>
      )}{" "}
      If another program has the database file open, close it. Then restart the server.
    </p>
  );
}
