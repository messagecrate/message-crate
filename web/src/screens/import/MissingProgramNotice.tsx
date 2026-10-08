import Button from "../../components/Button";
import type { ToolName, ToolsStatus } from "../../lib/tauri";
import { toolUsable } from "../../lib/tauri";
import { toolStatusLine, troubleshootingSection } from "../../lib/toolStatusCopy";
import { accentLink } from "../../lib/uiStyles";
import {
  OTHER_CHECK_POLL_FOR_MS,
  useRetryToolDownloads,
  useToolsStatus,
} from "../../lib/useToolsStatus";

/** What a program is needed for: a WhatsApp import, or Convert and Compress. */
export type ProgramNeed = "whatsapp" | "media";

/** The troubleshooting section for what `need` runs. */
function troubleshootingUrl(need: ProgramNeed): string {
  return troubleshootingSection(need === "whatsapp" ? "wtsexporter" : "ffmpeg").url;
}

/** `names` as a list in a sentence: "ffmpeg", or "ffmpeg and ffprobe". */
function nameList(names: readonly ToolName[]): string {
  return names.join(" and ");
}

/**
 * What the programs mean for the import: that it can't start or that Convert
 * and Compress can't run when one can't be used (`blocked`), or else that it
 * waits for the ones still downloading (`downloading`), named.
 */
function consequence(
  need: ProgramNeed,
  blocked: boolean,
  downloading: readonly ToolName[],
): string {
  if (blocked) {
    return need === "whatsapp"
      ? "A WhatsApp import can't start without wtsexporter."
      : "Convert and Compress need ffmpeg and ffprobe. Copy imports attachments as they are.";
  }
  const waits = nameList(downloading);
  return need === "whatsapp" ? `The import waits for ${waits}.` : `Media waits for ${waits}.`;
}

/**
 * Try again, which checks the Tools Directory and downloads what is missing
 * now rather than at the next start, and the troubleshooting section for
 * `need`.
 */
export function TryAgain({ need, offerRetry = true }: { need: ProgramNeed; offerRetry?: boolean }) {
  const retry = useRetryToolDownloads();
  // Try again started nothing because a check already holds the Tools
  // Directory: the start-up check, say, still on another program, or
  // another app's. `checking` counts only this process's checks, so the
  // status is also asked for every 2 s for two minutes, and this says why
  // nothing new started meanwhile.
  const alreadyRunning = retry.data === "alreadyRunning";
  useToolsStatus({
    otherCheckUntil: alreadyRunning ? retry.submittedAt + OTHER_CHECK_POLL_FOR_MS : 0,
  });
  return (
    <div className="mt-1 flex flex-wrap items-center gap-3">
      {offerRetry ? (
        <Button size="xs" onClick={() => retry.mutate()} disabled={retry.isPending}>
          Try again
        </Button>
      ) : null}
      <a href={troubleshootingUrl(need)} target="_blank" rel="noopener" className={accentLink}>
        Troubleshooting
      </a>
      {alreadyRunning ? (
        <span className="text-[0.813rem] text-muted">
          A check of the Tools Directory is already running. This form shows its result when it
          ends.
        </span>
      ) : null}
      {retry.isError ? (
        <span role="alert" className="text-[0.813rem] text-danger">
          The download didn't start.{" "}
          {retry.error instanceof Error ? retry.error.message : String(retry.error)}
        </span>
      ) : null}
    </div>
  );
}

/**
 * Says which of `programs` the chosen import needs and can't use, why (the
 * download failed, with its reason, or it hasn't been downloaded), and offers
 * Try again. A program still downloading is shown with its progress: the
 * import waits for it. Nothing is shown when every program is found.
 */
export function MissingProgramNotice({
  programs,
  status,
  need,
}: {
  programs: readonly ToolName[];
  status: ToolsStatus;
  need: ProgramNeed;
}) {
  const lines = programs.flatMap((name) => {
    const program = status[name];
    if (program.state === "found") return [];
    const { before, after } = toolStatusLine(program);
    return [`${before}${name}${after}`];
  });
  if (lines.length === 0) return null;
  const unusable = programs.filter((name) => !toolUsable(status[name]));
  const blocked = unusable.length > 0;
  const downloading = programs.filter((name) => status[name].state === "downloading");
  // A program the app has no download of for this computer can't be brought
  // by Try again: the troubleshooting section says where a copy goes.
  const retryable = unusable.some((name) => status[name].state !== "unavailable");
  return (
    <div role="status" className="mt-2 text-[0.813rem] text-muted">
      <p className="m-0">
        {lines.join(" ")} {consequence(need, blocked, downloading)}
      </p>
      {blocked ? <TryAgain need={need} offerRetry={retryable} /> : null}
    </div>
  );
}
