import { useState } from "react";
import { Header, ListBoxSection } from "react-aria-components";
import LogViewer from "../../components/logs/LogViewer";
import {
  runLogDownload,
  runLogSource,
  SERVER_LOG,
  UNREADABLE_RUN_LOG,
  useRunLogReader,
  useRunLogs,
  useServerLogDownloads,
} from "../../components/logs/logSource";
import Select, {
  ListBoxItem,
  selectItemClassName,
  selectSectionHeaderClass,
} from "../../components/Select";
import { rejectionMessage } from "../../lib/apiErrorMessage";
import { formatDateTime } from "../../lib/formatDate";
import type { RunLogEntry } from "../../lib/tauri";
import { sectionHintClass } from "../settings/storage/storageUtils";
import { useOwnerAccounts } from "./useOwnerAccounts";

const itemClassName = (state: { isFocused: boolean; isSelected: boolean }) =>
  selectItemClassName(state, "sm");

/** The picker's key for the server's log. */
const SERVER_KEY = "server";

/** The picker's key for an Import Run log. */
function runKey(name: string): string {
  return `run:${name}`;
}

/**
 * How the picker names an Import Run log: the run, who ran it, and when its
 * last line was written. An account of this Message Crate goes by its
 * username; one of another Message Crate by its number and that server's
 * address.
 */
function runLogLabel(log: RunLogEntry, usernames: ReadonlyMap<number, string>): string {
  const when = formatDateTime(log.modifiedAt);
  const account = log.account;
  if (!account) return `${log.name}, ${when}`;
  const who = log.thisMessageCrate
    ? (usernames.get(account.accountId) ?? `account ${account.accountId}`)
    : `account ${account.accountId} on ${account.server}`;
  return `Import Run ${account.importRunId} by ${who}, ${when}`;
}

/**
 * Owner Home's Logs: the server's log, and in the desktop app every Import
 * Run log on this computer, whoever ran the import (#1665). A run log stays
 * on the computer that ran the import, so in a browser the picker offers the
 * server's log alone. Each log reads in the same viewer.
 */
export function OwnerLogsPanel() {
  const [picked, setPicked] = useState(SERVER_KEY);
  const reader = useRunLogReader();
  const runs = useRunLogs(reader);
  const { accounts } = useOwnerAccounts();
  const usernames = new Map(accounts.map((account) => [account.account_id, account.username]));
  const serverDownloads = useServerLogDownloads();

  const pickedRun =
    reader && picked !== SERVER_KEY ? runs.logs.find((log) => runKey(log.name) === picked) : null;

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
            selectedKey={pickedRun ? picked : SERVER_KEY}
            onSelectionChange={(key) => {
              if (key != null) setPicked(String(key));
            }}
          >
            <ListBoxItem id={SERVER_KEY} className={itemClassName}>
              The server's log
            </ListBoxItem>
            {reader && runs.logs.length > 0 ? (
              <ListBoxSection>
                <Header className={selectSectionHeaderClass}>Import Runs on this computer</Header>
                {runs.logs.map((log) => {
                  const label = runLogLabel(log, usernames);
                  return (
                    <ListBoxItem
                      key={log.name}
                      id={runKey(log.name)}
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
        {pickedRun
          ? `${pickedRun.name}, kept in the Logs Directory on this computer. A log names counts, files and what went wrong, never what a message says.`
          : "What the server did, every account's requests in one place. A log names counts, routes and what went wrong, never what a message says."}
      </p>
      {runs.error ? (
        <p role="alert" className="mt-2 text-[0.813rem] text-danger">
          {rejectionMessage(runs.error, "Could not list the Import Run logs on this computer.")}
        </p>
      ) : null}
      <div className="mt-3">
        {pickedRun && reader ? (
          <LogViewer
            key={pickedRun.name}
            source={runLogSource(reader, pickedRun.name)}
            downloads={[runLogDownload(reader, pickedRun.name)]}
            unreadable={pickedRun.hasLines ? undefined : UNREADABLE_RUN_LOG}
          />
        ) : (
          <LogViewer key={SERVER_KEY} source={SERVER_LOG} downloads={serverDownloads.downloads} />
        )}
      </div>
    </section>
  );
}
