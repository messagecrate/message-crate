import { useState } from "react";
import Button from "../../../components/Button";
import LogViewer from "../../../components/logs/LogViewer";
import {
  runLogDownload,
  runLogSource,
  useRunLogReader,
  useRunLogs,
} from "../../../components/logs/logSource";
import { canReadImportRunLogs } from "../../../lib/desktopFeatures";
import { isTauri } from "../../../lib/tauri-check";

/**
 * An Import Run's log, from the run's row in Settings → Storage. The log
 * stays on the computer that ran the import, so only the desktop app on that
 * computer offers it, and only to the account that ran the run and the owner
 * (#1665). Anywhere else this shows nothing.
 */
export default function ImportRunLog({ importRunId }: { importRunId: number }) {
  if (!canReadImportRunLogs(isTauri())) return null;
  return <RunLogOnThisComputer importRunId={importRunId} />;
}

/** Whether two server addresses name one server, apart from a trailing slash. */
function sameServer(a: string, b: string): boolean {
  return a.replace(/\/+$/, "") === b.replace(/\/+$/, "");
}

function RunLogOnThisComputer({ importRunId }: { importRunId: number }) {
  const [open, setOpen] = useState(false);
  const reader = useRunLogReader();
  const { logs } = useRunLogs(reader);
  const log =
    reader &&
    logs.find(
      (entry) =>
        entry.account?.importRunId === importRunId &&
        sameServer(entry.account.server, reader.server),
    );
  if (!reader || !log) return null;
  return (
    <div className="mt-4">
      <div className="mb-2 flex flex-wrap items-center gap-2">
        <h4 className="m-0 font-medium text-[0.813rem]">Log</h4>
        <Button
          type="button"
          variant="ghost"
          size="chip"
          aria-expanded={open}
          onClick={() => setOpen((was) => !was)}
        >
          {open ? "Hide this run's log" : "Open this run's log"}
        </Button>
      </div>
      {open ? (
        <LogViewer
          source={runLogSource(reader, log.name)}
          downloads={[runLogDownload(reader, log.name)]}
        />
      ) : null}
    </div>
  );
}
