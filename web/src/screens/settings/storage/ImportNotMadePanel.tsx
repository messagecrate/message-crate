import { formatBytes } from "../../../lib/formatBytes";
import { countOf } from "../../../lib/plural";
import type { components } from "../../../lib/serverApi.types";
import { tdClass, thClass } from "./storageUtils";

type Schema = components["schemas"];
type NotMadeKind = Schema["NotMadeKind"];
type NotMadeCount = Schema["NotMadeCount"];
type NotMadeAttachment = Schema["NotMadeAttachment"];
type VersionNotMade = Schema["VersionNotMade"];

/** Each kind of failure as the owner reads it. */
const NOT_MADE_KIND_LABEL: Record<NotMadeKind, string> = {
  conversion_failed: "Conversion failed",
  stalled: "Stopped making progress",
  ran_past_length: "Damaged, ran past its length",
  took_too_long: "Took too long",
  original_missing: "Original file missing",
  server_problem: "Server problem",
};

/**
 * The attachments of one Import Run whose Thumbnail or Preview the server
 * could not make, as the owner reads them under another account: how many
 * for each kind of failure, then each attachment by its file name, type and
 * size, with why for each version not made. The server tries again only when
 * someone runs `process-assets`, so the panel offers no retry.
 */
export default function ImportNotMadePanel({
  attachmentCount,
  counts,
  attachments,
}: {
  /** The attachments counted for the run, which the failures are out of. */
  attachmentCount: number;
  counts: NotMadeCount[];
  attachments: NotMadeAttachment[];
}) {
  return (
    <div className="mt-4">
      <h4 className="mb-1 font-medium text-[0.813rem]">Thumbnails and Previews</h4>
      {attachments.length === 0 ? (
        <p className="text-[0.813rem] text-text">
          No attachment of this import is missing a Thumbnail or Preview the server tried to make.
        </p>
      ) : (
        <>
          <p className="mb-2 text-[0.813rem] text-text">
            {attachments.length.toLocaleString()} of {countOf(attachmentCount, "attachment")}{" "}
            {attachments.length === 1 ? "has" : "have"} no Thumbnail or Preview
          </p>
          <table className="mb-3 border-collapse">
            <thead>
              <tr>
                <th className={thClass}>Kind of failure</th>
                <th className={`${thClass} text-right`}>No Thumbnail</th>
                <th className={`${thClass} text-right`}>No Preview</th>
              </tr>
            </thead>
            <tbody>
              {counts.map((count) => (
                <tr key={count.kind}>
                  <td className={tdClass}>{NOT_MADE_KIND_LABEL[count.kind]}</td>
                  <td className={`${tdClass} text-right tabular-nums`}>
                    {count.thumbnails.toLocaleString()}
                  </td>
                  <td className={`${tdClass} text-right tabular-nums`}>
                    {count.previews.toLocaleString()}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          <div className="max-h-80 overflow-y-auto rounded-lg border border-border">
            <table className="w-full border-collapse">
              <thead>
                <tr>
                  <th className={thClass}>Name</th>
                  <th className={thClass}>Type</th>
                  <th className={`${thClass} text-right`}>Size</th>
                  <th className={thClass}>Thumbnail</th>
                  <th className={thClass}>Preview</th>
                </tr>
              </thead>
              <tbody>
                {attachments.map((attachment) => (
                  <tr key={attachment.id} className="align-top">
                    <td className={`${tdClass} min-w-[8rem] max-w-[14rem] wrap-anywhere`}>
                      {attachment.original_name || `Attachment ${attachment.id}`}
                    </td>
                    <td className={tdClass}>{attachment.mime_type ?? "Unknown"}</td>
                    <td className={`${tdClass} whitespace-nowrap text-right tabular-nums`}>
                      {attachment.size_bytes == null ? "—" : formatBytes(attachment.size_bytes)}
                    </td>
                    <td className={tdClass}>
                      <VersionNotMadeCell version={attachment.thumbnail} />
                    </td>
                    <td className={tdClass}>
                      <VersionNotMadeCell version={attachment.preview} />
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </>
      )}
    </div>
  );
}

/** Why one version was not made, or a dash when nothing failed. */
function VersionNotMadeCell({ version }: { version: VersionNotMade | null }) {
  if (!version) return <span className="text-muted">—</span>;
  return (
    <>
      <span className="block">{NOT_MADE_KIND_LABEL[version.kind]}</span>
      <span className="block whitespace-pre-wrap wrap-anywhere text-muted">{version.reason}</span>
    </>
  );
}
