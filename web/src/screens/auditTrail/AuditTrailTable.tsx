import { Link } from "react-router-dom";
import ScrollingTableCard from "../../components/ScrollingTableCard";
import { AUDIT_TRAIL_PAGE_SIZE } from "../../hooks/useAuditTrail";
import { type AuditEntry, auditActorLabel, describeAuditEntry } from "../../lib/auditTrail";
import { formatDateTime } from "../../lib/formatDate";
import { accentLinkClass } from "../../lib/uiStyles";
import PageControl from "../settings/storage/PageControl";
import {
  sectionHintClass,
  tableCardClass,
  tdClass,
  thClass,
} from "../settings/storage/storageUtils";

/** The Account column: the username, marked when the account has since been deleted. */
function accountCell(entry: AuditEntry) {
  // Opening and closing the Message Crate to new accounts is about no account,
  // and a refused login that typed no valid username keeps none.
  if (!entry.username) return <span className="text-muted">—</span>;
  return (
    <>
      {entry.username}
      {entry.account_deleted ? (
        <span className="block text-[0.75rem] text-muted">deleted</span>
      ) : null}
    </>
  );
}

/**
 * One page of an Audit Trail, newest first: when, what, and who acted, with
 * the account each entry is about when the table lists more than one
 * account. Every entry says what happened and how much, never what a message
 * said.
 *
 * Given `logHref`, an entry written in a request links to that request's
 * lines in the server's log. Only Owner Home passes it: a holder reading
 * their own Audit Trail in Settings cannot read the log.
 */
export default function AuditTrailTable({
  entries,
  total,
  page,
  onPageChange,
  showAccount,
  logHref,
}: {
  /** The entries on this page. */
  entries: AuditEntry[];
  /** How many entries the trail holds, across every page. */
  total: number;
  page: number;
  onPageChange: (page: number) => void;
  /** Show the Account column: the owner's list of every account. */
  showAccount: boolean;
  /** Where an entry's request's lines in the server's log are read, by its `request_id`. */
  logHref?: (requestId: string) => string;
}) {
  if (total === 0) {
    return <p className={`${sectionHintClass} mt-3`}>Nothing recorded yet</p>;
  }
  return (
    <div className="mt-3 flex flex-col gap-3">
      <ScrollingTableCard cardClassName={tableCardClass}>
        <table className="w-full border-collapse">
          <thead>
            <tr>
              <th className={thClass}>Date</th>
              {showAccount ? <th className={thClass}>Account</th> : null}
              <th className={thClass}>What happened</th>
              <th className={thClass}>By</th>
              {logHref ? (
                <th className={thClass}>
                  <span className="sr-only">Log</span>
                </th>
              ) : null}
            </tr>
          </thead>
          <tbody>
            {entries.map((entry) => (
              <tr key={`${entry.action}-${entry.id}`}>
                <td className={`${tdClass} whitespace-nowrap`}>{formatDateTime(entry.at)}</td>
                {showAccount ? <td className={tdClass}>{accountCell(entry)}</td> : null}
                <td className={tdClass}>
                  {/* The card sizes to its content, so a long line wraps here
                      rather than pushing the By column out of view. */}
                  <div className="max-w-[30rem]">{describeAuditEntry(entry)}</div>
                </td>
                <td className={`${tdClass} whitespace-nowrap`}>{auditActorLabel(entry.actor)}</td>
                {logHref ? (
                  <td className={`${tdClass} whitespace-nowrap`}>
                    {/* An entry written by a server command, a run or a
                        Session that ran out has no request. */}
                    {entry.request_id ? (
                      <Link to={logHref(entry.request_id)} className={accentLinkClass}>
                        View in log
                      </Link>
                    ) : null}
                  </td>
                ) : null}
              </tr>
            ))}
          </tbody>
        </table>
      </ScrollingTableCard>
      <PageControl
        page={page}
        total={total}
        pageSize={AUDIT_TRAIL_PAGE_SIZE}
        onPageChange={onPageChange}
      />
    </div>
  );
}
