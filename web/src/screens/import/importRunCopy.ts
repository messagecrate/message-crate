import type { ImportSummaryView } from "../../components/import/ImportSummaryPanel";
import { sourceLabel } from "../../lib/exportSources";
import { IMPORT_SOURCES } from "../../lib/importSources";
import { countOf } from "../../lib/plural";
import { ATTACHMENT_OPTIONS } from "./ImportFormUi";
import type { ImportPhase } from "./importProgressState";
import type { ImportJobFormValues } from "./useImportJob";

/** Which of the run's two reviews ((CONTEXT.md, "Review")). */
export type ReviewKind = "staging" | "media";

/**
 * The person's name for the source: the source's label for a source read
 * one way, e.g. "SMS Backup & Restore", and the label and the method's
 * otherwise, e.g. "Apple Messages · iPhone backup".
 */
export function sourceDisplayName(source: string): string {
  for (const descriptor of IMPORT_SOURCES) {
    const method = descriptor.methods.find((m) => m.id === source);
    if (method === undefined) continue;
    return descriptor.methods.length === 1
      ? descriptor.label
      : `${descriptor.label} · ${method.label}`;
  }
  return sourceLabel(source);
}

/** The attachments line of "what you asked for", in the form's own words, settings included when they apply. */
export function attachmentsAsked(form: ImportJobFormValues): string {
  const label =
    ATTACHMENT_OPTIONS.find((o) => o.id === form.attachmentMedia)?.label ?? form.attachmentMedia;
  if (form.attachmentMedia !== "convert" && form.attachmentMedia !== "compress") return label;
  return `${label} · up to ${form.maxResolution}, ${form.maxFps} fps, files over ${form.minSizeMb} MB`;
}

/** The run view's heading: what the run is doing, or what it did. */
export function runHeading(
  phase: ImportPhase,
  form: ImportJobFormValues | null,
  summaryView: ImportSummaryView | null,
  completionText: string | undefined,
): string {
  if (phase !== "done") {
    return form ? `Importing from ${sourceDisplayName(form.source)}` : "Importing";
  }
  const status = summaryView?.status;
  const inserted = summaryView?.messagesInserted;
  if ((status === "completed" || status === "completed_with_issues") && inserted != null) {
    const imported = `Imported ${countOf(inserted, "message")}`;
    return status === "completed_with_issues" ? `${imported}, with errors` : imported;
  }
  return completionText ?? "Import finished";
}

/**
 * Name of the Contact Group the server made for a finished run: the same
 * words `import_contact_group_name` (server) uses, so a link lands on it.
 */
export function importGroupName(
  source: string,
  finishedAt: string | null | undefined,
  startedAt: string,
): string {
  const date = (finishedAt ?? startedAt).slice(0, 10);
  return `${source} import ${date}`;
}
