import { useState } from "react";
import { Header, ListBoxSection } from "react-aria-components";
import LogViewer from "../../components/logs/LogViewer";
import {
  desktopLogDownload,
  desktopLogSource,
  SERVER_LOG,
  UNREADABLE_RUN_LOG,
} from "../../components/logs/logSource";
import Select, {
  ListBoxItem,
  selectItemClassName,
  selectSectionHeaderClass,
} from "../../components/Select";
import { useDesktopLogs } from "../../hooks/useDesktopLogs";
import { useLogReader } from "../../hooks/useLogReader";
import { useOwnerAccounts } from "../../hooks/useOwnerAccounts";
import { errorLine } from "../../lib/apiErrorMessage";
import type { LogEntry } from "../../lib/desktop/runLog";
import { formatDateTime } from "../../lib/formatDate";
import { sectionHintClass } from "../settings/storage/storageUtils";
import { useServerLogDownloads } from "./useServerLogDownloads";

const itemClassName = (state: { isFocused: boolean; isSelected: boolean }) =>
  selectItemClassName(state, "sm");

/** The picker's key for the server's log. */
const SERVER_KEY = "server";

/** What the hint under the picker says of the server's log. */
const SERVER_LOG_HINT =
  "What the server did, every account's requests in one place. A log names counts, routes and what went wrong, never what a message says.";

/** What the hint under the picker says of each kind of log on this computer, after its name. */
const LOG_HINTS: Record<LogEntry["kind"], string> = {
  importRun: "A log names counts, files and what went wrong, never what a message says.",
  desktopApp:
    "What the desktop app went on without outside an Import Run, such as a download it could not record.",
};

/** The picker's key for a log on this computer. */
function logKey(name: string): string {
  return `log:${name}`;
}

/**
 * How the picker names an Import Run log: the run, who ran it, and when its
 * last line was written. An account of this Message Crate goes by its
 * username; one of another Message Crate by its number and that server's
 * address.
 */
function runLogLabel(log: LogEntry, usernames: ReadonlyMap<number, string>): string {
  const when = formatDateTime(log.modifiedAt);
  const account = log.account;
  if (!account) return `${log.name}, ${when}`;
  const who = log.thisMessageCrate
    ? (usernames.get(account.accountId) ?? `account ${account.accountId}`)
    : `account ${account.accountId} on ${account.server}`;
  return `Import Run ${account.importRunId} by ${who}, ${when}`;
}

/**
 * Owner Home's Logs: the server's log, and in the desktop app the desktop
 * app's own log (#2185) and every Import Run log on this computer, whoever
 * ran the import (#1665). Both stay on this computer, so in a browser the
 * picker offers the server's log alone. Each log reads in the same viewer.
 *
 * Given `request`, the server's log opens at every level with the request's
 * id in the search, so it shows that request's lines alone: an Audit Trail
 * entry's "View in log" opens it so (#3060).
 */
export function OwnerLogsPanel({ request }: { request?: string } = {}) {
  const [picked, setPicked] = useState(SERVER_KEY);
  const reader = useLogReader();
  const onComputer = useDesktopLogs(reader);
  const { accounts } = useOwnerAccounts();
  const usernames = new Map(accounts.map((account) => [account.account_id, account.username]));
  const serverDownloads = useServerLogDownloads();

  const pickedLog =
    reader && picked !== SERVER_KEY
      ? onComputer.logs.find((log) => logKey(log.name) === picked)
      : null;
  const desktopApp = onComputer.logs.find((log) => log.kind === "desktopApp");
  const importRuns = onComputer.logs.filter((log) => log.kind === "importRun");

  return (
    <section>
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h3 className="m-0 text-text">Logs</h3>
        <div className="flex items-center gap-2 text-[0.813rem] text-muted">
          <span aria-hidden="true">Log</span>
          <Select
            aria-label="Log"
            size="sm"
            className="w-[22rem] max-w-full"
            selectedKey={pickedLog ? picked : SERVER_KEY}
            onSelectionChange={(key) => {
              if (key != null) setPicked(String(key));
            }}
          >
            <ListBoxItem id={SERVER_KEY} className={itemClassName}>
              The server's log
            </ListBoxItem>
            {reader && desktopApp ? (
              <ListBoxItem id={logKey(desktopApp.name)} className={itemClassName}>
                Desktop app
              </ListBoxItem>
            ) : null}
            {reader && importRuns.length > 0 ? (
              <ListBoxSection>
                <Header className={selectSectionHeaderClass}>Import Runs on this computer</Header>
                {importRuns.map((log) => {
                  const label = runLogLabel(log, usernames);
                  return (
                    <ListBoxItem
                      key={log.name}
                      id={logKey(log.name)}
                      textValue={label}
                      className={itemClassName}
                    >
                      {label}
                    </ListBoxItem>
                  );
                })}
              </ListBoxSection>
            ) : null}
          </Select>
        </div>
      </div>
      <p className={sectionHintClass}>
        {pickedLog
          ? `${pickedLog.name}, kept in the Logs Directory on this computer. ${LOG_HINTS[pickedLog.kind]}`
          : SERVER_LOG_HINT}
      </p>
      {onComputer.error ? (
        <p role="alert" className="mt-2 text-[0.813rem] text-danger">
          {errorLine(onComputer.error, "Could not list the logs on this computer.")}
        </p>
      ) : null}
      <div className="mt-3">
        {pickedLog && reader ? (
          <LogViewer
            key={pickedLog.name}
            source={desktopLogSource(reader, pickedLog.name)}
            downloads={[desktopLogDownload(reader, pickedLog.name)]}
            unreadable={pickedLog.hasLines ? undefined : UNREADABLE_RUN_LOG}
          />
        ) : (
          <LogViewer
            key={`${SERVER_KEY}:${request ?? ""}`}
            source={SERVER_LOG}
            downloads={serverDownloads.downloads}
            request={request}
          />
        )}
      </div>
    </section>
  );
}
