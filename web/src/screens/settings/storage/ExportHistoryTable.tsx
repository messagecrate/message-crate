import ScrollingTableCard from "../../../components/ScrollingTableCard";
import { formatBytes } from "../../../lib/formatBytes";
import PageControl from "./PageControl";
import type { ListedExportRun } from "./storageUtils";
import {
  describeExportRun,
  formatImportDate,
  RUN_PAGE_SIZE,
  sectionHintClass,
  sectionTitleClass,
  tableCardClass,
  tdClass,
  thClass,
} from "./storageUtils";

/**
 * The word the table shows for a run's status. Every status is named: one the
 * server adds fails the type-check at `satisfies never` until it has a case.
 */
function statusLabel(status: ListedExportRun["status"]): string {
  switch (status) {
    case "running":
      return "Running";
    case "completed":
      return "Completed";
    case "failed":
      return "Failed";
    case "cancelled":
      return "Cancelled";
    default:
      status satisfies never;
      return status;
  }
}

/**
 * Every Export Run recorded for the account, one page at a time: what was
 * asked for and how much matched, never what the messages said. A run that
 * never finished shows how far it got in the Delivered column.
 */
export default function ExportHistoryTable({
  exports,
  total,
  page,
  onPageChange,
}: {
  /** The runs on this page. */
  exports: ListedExportRun[];
  /** How many runs the account has, across every page. */
  total: number;
  page: number;
  onPageChange: (page: number) => void;
}) {
  return (
    <section>
      <h3 className={sectionTitleClass}>Export history</h3>
      <p className={sectionHintClass}>
        Each export recorded for this account
        {total > RUN_PAGE_SIZE ? ` · ${RUN_PAGE_SIZE} per page` : ""}.
      </p>
      {total === 0 ? (
        <p className={`${sectionHintClass} mt-3`}>No exports recorded yet</p>
      ) : (
        <div className="mt-3 flex flex-col gap-3">
          <ScrollingTableCard cardClassName={tableCardClass}>
            <table className="w-full border-collapse">
              <thead>
                <tr>
                  <th className={thClass}>Date</th>
                  <th className={thClass}>Scope</th>
                  <th className={thClass}>Status</th>
                  <th className={`${thClass} text-right`}>Messages</th>
                  <th className={`${thClass} text-right`}>Delivered</th>
                  <th className={`${thClass} text-right`}>Attachments</th>
                  <th className={`${thClass} text-right`}>Size</th>
                </tr>
              </thead>
              <tbody>
                {exports.map((row) => (
                  <tr key={row.id}>
                    <td className={tdClass}>
                      {formatImportDate(row.finished_at ?? row.started_at)}
                    </td>
                    <td className={tdClass}>{describeExportRun(row)}</td>
                    <td className={tdClass}>{statusLabel(row.status)}</td>
                    <td className={`${tdClass} text-right tabular-nums`}>
                      {row.message_count.toLocaleString()}
                    </td>
                    <td className={`${tdClass} text-right tabular-nums`}>
                      {row.messages_delivered.toLocaleString()}
                    </td>
                    <td className={`${tdClass} text-right tabular-nums`}>
                      {row.attachment_count.toLocaleString()}
                    </td>
                    <td className={`${tdClass} text-right tabular-nums`}>
                      {formatBytes(row.total_bytes)}
                    </td>
                  </tr>
                ))}
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
