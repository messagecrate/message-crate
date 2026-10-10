import ScrollingTableCard from "../../../components/ScrollingTableCard";
import { formatBytes } from "../../../lib/formatBytes";
import PageControl from "./PageControl";
import type { TopAttachment } from "./storageUtils";
import {
  ATTACHMENT_PAGE_SIZE,
  sectionHintClass,
  sectionTitleClass,
  tableCardClass,
  tdClass,
  thClass,
} from "./storageUtils";

export default function TopAttachmentsTable({
  topAttachments,
  page,
  onPageChange,
  showConversation,
}: {
  topAttachments: TopAttachment[];
  page: number;
  onPageChange: (page: number) => void;
  /** False for the owner, whom the server does not tell which conversation a file is in. */
  showConversation: boolean;
}) {
  const pageRows = topAttachments.slice(
    page * ATTACHMENT_PAGE_SIZE,
    page * ATTACHMENT_PAGE_SIZE + ATTACHMENT_PAGE_SIZE,
  );

  return (
    <section>
      <h3 className={sectionTitleClass}>Largest attachments</h3>
      <p className={sectionHintClass}>
        Top {topAttachments.length || 100} attachments by file size
        {topAttachments.length > ATTACHMENT_PAGE_SIZE ? ` · ${ATTACHMENT_PAGE_SIZE} per page` : ""}.
      </p>
      {topAttachments.length === 0 ? (
        <p className={`${sectionHintClass} mt-3`}>No attachments with sizes yet</p>
      ) : (
        <div className="mt-3 flex flex-col gap-3">
          <ScrollingTableCard cardClassName={tableCardClass}>
            <table className="w-full border-collapse">
              <thead>
                <tr>
                  <th className={thClass}>Name</th>
                  {showConversation ? <th className={thClass}>Conversation</th> : null}
                  <th className={`${thClass} text-right`}>Size</th>
                </tr>
              </thead>
              <tbody>
                {pageRows.map((row) => (
                  <tr key={row.id}>
                    <td className={`${tdClass} max-w-[14rem] truncate`}>
                      {row.original_name || row.mime_type || `Attachment ${row.id}`}
                    </td>
                    {showConversation ? (
                      <td className={tdClass}>{row.group_title || row.chat_identifier}</td>
                    ) : null}
                    <td className={`${tdClass} text-right tabular-nums`}>
                      {formatBytes(row.size_bytes)}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </ScrollingTableCard>
          <PageControl
            page={page}
            total={topAttachments.length}
            pageSize={ATTACHMENT_PAGE_SIZE}
            onPageChange={onPageChange}
          />
        </div>
      )}
    </section>
  );
}
