import type { ImportJobFormValues } from "../../../lib/importSources/types";
import type {
  AttachmentForecast,
  OwnerIdentityCount,
  SizeVerdict,
  StagingSummary,
} from "../../../lib/tauri";
import { isAttachmentMediaMode, isStringArray } from "../formSnapshot";
import { importRunStore as store } from "../importRunStore";
import { runScratch } from "./scratch";

const SIZE_VERDICTS: readonly SizeVerdict[] = [
  "fits_as_is",
  "likely_fits",
  "may_grow",
  "probably_too_big",
  "cannot_process",
];

function isAttachmentForecast(value: unknown): value is AttachmentForecast {
  if (typeof value !== "object" || value === null) return false;
  const r = value as Record<string, unknown>;
  return (
    typeof r.path === "string" &&
    typeof r.name === "string" &&
    typeof r.sizeBytes === "number" &&
    typeof r.estimateBytes === "number" &&
    typeof r.verdict === "string" &&
    SIZE_VERDICTS.includes(r.verdict as SizeVerdict)
  );
}

function isOwnerIdentityCount(value: unknown): value is OwnerIdentityCount {
  if (typeof value !== "object" || value === null) return false;
  const r = value as Record<string, unknown>;
  return (
    typeof r.identity === "string" && typeof r.sent === "number" && typeof r.received === "number"
  );
}

/**
 * Parse a run's stored `summary` back into a `StagingSummary`
 * — the plan approved at the last Review the run left.
 *
 * Read only as the *approved baseline* on resume, never shown directly:
 * the summary actually on screen is always recomputed
 * fresh from the directory. Like `restoreFormFromSnapshot`, this value came
 * from the database rather than from this run's own state, so its
 * shape is checked field by field rather than trusted; returns `undefined`
 * — not a throw — for anything that doesn't match. A resume with no usable
 * baseline still proceeds: `importOutcome` tolerates an absent one, it just
 * can't diff against one.
 */
export function parseStoredStagingSummary(raw: unknown): StagingSummary | undefined {
  if (typeof raw !== "object" || raw === null) return undefined;
  const r = raw as Record<string, unknown>;
  if (typeof r.conversations !== "number") return undefined;
  if (typeof r.messages !== "number") return undefined;
  if (!isStringArray(r.contactIdentifiers)) return undefined;
  if (!Array.isArray(r.ownerIdentities) || !r.ownerIdentities.every(isOwnerIdentityCount)) {
    return undefined;
  }
  if (typeof r.attachments !== "number") return undefined;
  if (typeof r.attachmentBytes !== "number") return undefined;
  if (!Array.isArray(r.forecasts) || !r.forecasts.every(isAttachmentForecast)) return undefined;
  if (typeof r.assetMaxBytes !== "number") return undefined;
  if (!isAttachmentMediaMode(r.mediaMode)) return undefined;

  return {
    conversations: r.conversations,
    messages: r.messages,
    contactIdentifiers: r.contactIdentifiers,
    ownerIdentities: r.ownerIdentities,
    attachments: r.attachments,
    attachmentBytes: r.attachmentBytes,
    forecasts: r.forecasts,
    assetMaxBytes: r.assetMaxBytes,
    mediaMode: r.mediaMode,
  };
}

/**
 * The run's form with the attachment mode its run directory recorded.
 *
 * Staging records the run's media settings in the directory, and the summary
 * of the directory carries the mode. From the end of Staging on, the directory is
 * the one source of the mode, so whether the run has a Media stage is read
 * from there and never from the stored form. `summary` is absent before
 * Staging has finished, and when a resumed run's stored plan no longer
 * parses; the form's own mode stands then, because nothing else holds one.
 */
export function withRecordedMode(
  form: ImportJobFormValues,
  summary: StagingSummary | null | undefined,
): ImportJobFormValues {
  if (!summary) return form;
  return { ...form, attachmentMedia: summary.mediaMode };
}

/**
 * `withRecordedMode`, made the run's own form: every later stage, the
 * progress rows and the review screens read the directory's mode from here.
 */
export function adoptRecordedMode(
  form: ImportJobFormValues,
  summary: StagingSummary | null | undefined,
): ImportJobFormValues {
  const recorded = withRecordedMode(form, summary);
  const scratch = runScratch();
  scratch.form = recorded;
  scratch.attachmentMode = recorded.attachmentMedia;
  store.set({ form: recorded });
  return recorded;
}
