import { Fragment } from "react";
import type { ImportSummaryView } from "../../../components/import/ImportSummaryPanel";
import PlainButton from "../../../components/PlainButton";
import ScrollingTableCard from "../../../components/ScrollingTableCard";
import { formatBytes } from "../../../lib/formatBytes";
import { focusRingClass } from "../../../lib/uiStyles";
import ImportDetailPanel from "./ImportDetailPanel";
import PageControl from "./PageControl";
import type { AccountImportRun, ListedImportRun } from "./storageUtils";
import {
  formatImportDate,
  RUN_PAGE_SIZE,
  sectionHintClass,
  sectionTitleClass,
  tableCardClass,
  tdClass,
  thClass,
} from "./storageUtils";

export default function ImportHistoryTable({
  imports,
  total,
  page,
  onPageChange,
  selectedImportId,
  selectedImport,
  selectedImportSummary,
  selectedImportLoading,
  selectedImportError,
  listContacts,
  onToggle,
  onCloseDetail,
}: {
  /** The runs on this page. */
  imports: ListedImportRun[];
  /** How many runs the account has, across every page. */
  total: number;
  page: number;
  onPageChange: (page: number) => void;
  listContacts: boolean;
  selectedImportId: number | null;
  selectedImport: AccountImportRun | null;
  selectedImportSummary: ImportSummaryView | null;
  selectedImportLoading: boolean;
  selectedImportError: string;
  onToggle: (importId: number) => void;
  onCloseDetail: () => void;
}) {
  return (
    <section>
      <h3 className={sectionTitleClass}>Import history</h3>
      <p className={sectionHintClass}>
        Each import recorded for this account, from the desktop app or the command line
        {total > RUN_PAGE_SIZE ? ` · ${RUN_PAGE_SIZE} per page` : ""}.
      </p>
      {total === 0 ? (
        <p className={`${sectionHintClass} mt-3`}>No imports recorded yet</p>
      ) : (
        <div className="mt-3 flex flex-col gap-3">
          <ScrollingTableCard cardClassName={tableCardClass}>
            <table className="w-full border-collapse">
              <thead>
                <tr>
                  <th className={thClass}>Date</th>
                  <th className={thClass}>Import type</th>
                  <th className={`${thClass} text-right`}>Messages</th>
                  <th className={`${thClass} text-right`}>Attachments</th>
                  <th className={`${thClass} text-right`}>Uploaded size</th>
                  <th className={`${thClass} text-right`}>Issues</th>
                </tr>
              </thead>
              <tbody>
                {imports.map((row) => {
                  const isSelected = selectedImportId === row.id;
                  const detailId = `import-detail-${row.id}`;
                  return (
                    <Fragment key={row.id}>
                      <tr
                        className={`cursor-pointer ${isSelected ? "bg-hover" : "hover:bg-hover"}`}
                        onClick={() => onToggle(row.id)}
                      >
                        <td className={tdClass}>
                          <PlainButton
                            aria-expanded={isSelected}
                            aria-controls={detailId}
                            // React Aria stops the press here, so the row's own click does not toggle it back.
                            onPress={() => onToggle(row.id)}
                            className={`w-full rounded-sm text-left ${focusRingClass}`}
                          >
                            {formatImportDate(row.finished_at ?? row.started_at)}
                          </PlainButton>
                        </td>
                        <td className={tdClass}>{row.source}</td>
                        <td className={`${tdClass} text-right tabular-nums`}>
                          {row.message_count.toLocaleString()}
                        </td>
                        <td className={`${tdClass} text-right tabular-nums`}>
                          {row.attachment_count.toLocaleString()}
                        </td>
                        <td className={`${tdClass} text-right tabular-nums`}>
                          {formatBytes(row.bytes_uploaded)}
                        </td>
                        <td className={`${tdClass} text-right tabular-nums`}>
                          {row.issue_count.toLocaleString()}
                        </td>
                      </tr>
                      {isSelected ? (
                        <tr>
                          <td colSpan={6} className="border-b border-border p-0">
                            <ImportDetailPanel
                              detailId={detailId}
                              selectedImport={selectedImport}
                              selectedImportSummary={selectedImportSummary}
                              selectedImportLoading={selectedImportLoading}
                              selectedImportError={selectedImportError}
                              listContacts={listContacts}
                              onClose={onCloseDetail}
                            />
                          </td>
                        </tr>
                      ) : null}
                    </Fragment>
                  );
                })}
              </tbody>
            </table>
          </ScrollingTableCard>
          <PageControl
            page={page}
            total={total}
            pageSize={RUN_PAGE_SIZE}
            onPageChange={onPageChange}
          />
        </div>
      )}
    </section>
  );
}
