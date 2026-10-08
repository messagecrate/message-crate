import ImportSummaryPanel, {
  type ImportSummaryView,
} from "../../../components/import/ImportSummaryPanel";
import PlainButton from "../../../components/PlainButton";
import ImportContactsPanel from "./ImportContactsPanel";
import ImportRunLog from "./ImportRunLog";
import type { AccountImportRun } from "./storageUtils";
import {
  formatBytes,
  formatImportDate,
  importBackup,
  importStatusLabel,
  sectionHint,
  sectionTitle,
} from "./storageUtils";

export default function ImportDetailPanel({
  detailId,
  selectedImport,
  selectedImportSummary,
  selectedImportLoading,
  selectedImportError,
  listContacts,
  onClose,
}: {
  detailId: string;
  /** False for the owner: who an account's contacts are is the account's own. */
  listContacts: boolean;
  selectedImport: AccountImportRun | null;
  selectedImportSummary: ImportSummaryView | null;
  selectedImportLoading: boolean;
  selectedImportError: string;
  onClose: () => void;
}) {
  return (
    // contain-inline-size: the panel takes the width the history table's columns give it,
    // and its content adds nothing to that width. The panel sits in a colSpan cell of the
    // history table, and the table's card is as wide as the table's content. Without it,
    // the card grows to the panel's content. In Chrome the summary's fixed-layout table has
    // a max-content width of 1,000,000 px. React Aria sizes the errors and notes tables to
    // their own box, so they then grow every frame, and since React Aria draws only the
    // columns inside the box's scroll view, each shows only its first column (#1708).
    <div id={detailId} className="bg-surface p-4 contain-inline-size">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div>
          <h3 className={sectionTitle}>Import details</h3>
          {selectedImport ? (
            <div className="mt-2 flex flex-wrap gap-2 text-[0.75rem]">
              <span className="rounded-full border border-border bg-elevated px-2.5 py-1 text-text">
                Type: {selectedImport.source}
              </span>
              <span className="rounded-full border border-border bg-elevated px-2.5 py-1 capitalize text-text">
                Mode: {selectedImport.mode}
              </span>
              <span className="rounded-full border border-border bg-elevated px-2.5 py-1 text-text">
                Status: {importStatusLabel(selectedImport.status)}
              </span>
            </div>
          ) : (
            <p className={sectionHint}>Loading import details…</p>
          )}
        </div>
        <PlainButton
          aria-label="Close import details"
          title="Close import details"
          onPress={onClose}
          className="flex size-8 items-center justify-center rounded-md text-xl leading-none text-muted hover:bg-hover hover:text-text"
        >
          ×
        </PlainButton>
      </div>

      {selectedImportLoading ? (
        <p className="mt-4 text-[0.813rem] text-muted">Loading import summary…</p>
      ) : null}

      {selectedImportError ? (
        <div className="mt-4 rounded-md border border-danger-soft-border bg-danger-soft-bg p-2 px-3 text-[0.813rem] text-danger">
          {selectedImportError}
        </div>
      ) : null}

      {selectedImport && selectedImportSummary ? (
        <>
          <dl className="mt-4 grid gap-3 text-[0.813rem] text-text sm:grid-cols-2 lg:grid-cols-3">
            <div>
              <dt className="text-muted">Started</dt>
              <dd className="mt-1">{formatImportDate(selectedImport.started_at)}</dd>
            </div>
            <div>
              <dt className="text-muted">Finished</dt>
              <dd className="mt-1">
                {selectedImport.finished_at
                  ? formatImportDate(selectedImport.finished_at)
                  : "Not finished"}
              </dd>
            </div>
            <div>
              <dt className="text-muted">Messages</dt>
              <dd className="mt-1">{selectedImport.message_count.toLocaleString()}</dd>
            </div>
            <div>
              <dt className="text-muted">Attachments</dt>
              <dd className="mt-1">{selectedImport.attachment_count.toLocaleString()}</dd>
            </div>
            <div>
              <dt className="text-muted">Bytes uploaded</dt>
              <dd className="mt-1">{formatBytes(selectedImport.bytes_uploaded)}</dd>
            </div>
            <div>
              <dt className="text-muted">Issues</dt>
              <dd className="mt-1">{selectedImport.issue_count.toLocaleString()}</dd>
            </div>
            <ImportBackupDetail run={selectedImport} />
          </dl>
          <ImportSummaryPanel summary={selectedImportSummary} />
          <div className="mt-4">
            <h4 className="mb-1 font-medium text-[0.813rem]">Contacts</h4>
            {listContacts ? (
              <ImportContactsPanel
                importId={selectedImport.id}
                newCount={selectedImport.contacts_new}
                changedCount={selectedImport.contacts_changed}
              />
            ) : (
              <p className="text-[0.813rem] text-text">
                {selectedImport.contacts_new.toLocaleString()} new,{" "}
                {selectedImport.contacts_changed.toLocaleString()} changed
              </p>
            )}
          </div>
          <ImportRunLog importRunId={selectedImport.id} />
        </>
      ) : null}
    </div>
  );
}

/**
 * The backup the run read, beside when that backup was made, so two imports of
 * one phone can be told apart. The owner's view of another account's run
 * carries neither, and shows nothing here.
 */
function ImportBackupDetail({ run }: { run: AccountImportRun }) {
  const backup = importBackup(run);
  if (!backup) return null;
  return (
    <div className="sm:col-span-2 lg:col-span-3">
      <dt className="text-muted">Backup</dt>
      <dd className="mt-1 break-all">{backup.file ?? "Not recorded"}</dd>
      <dd className="mt-1">
        {backup.takenAt
          ? `Made ${formatImportDate(backup.takenAt)}`
          : "The backup does not say when it was made"}
      </dd>
    </div>
  );
}
