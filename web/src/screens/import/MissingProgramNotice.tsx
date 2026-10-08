import Button from "../../components/Button";
import type { ToolStatus, ToolsStatus } from "../../lib/tauri";
import { toolUsable } from "../../lib/tauri";
import { downloadProgress } from "../../lib/toolStatusCopy";
import { accentLink } from "../../lib/uiStyles";
import { useRetryToolDownloads, useToolsStatus } from "../../lib/useToolsStatus";

/** A program the Import form can need. */
export type ProgramName = "ffmpeg" | "ffprobe" | "wtsexporter";

/** What a program is needed for: a WhatsApp import, or Convert and Compress. */
export type ProgramNeed = "whatsapp" | "media";

const TROUBLESHOOTING = "https://messagecrate.app/docs/user/features/owner/troubleshooting/";

/** The troubleshooting section for what `need` runs. */
function troubleshootingUrl(need: ProgramNeed): string {
  return `${TROUBLESHOOTING}#${need === "whatsapp" ? "import-cant-find-wtsexporter" : "ffmpeg-or-ffprobe-not-found"}`;
}

/** One program's state, as a sentence; null when it is found. */
function programLine(name: ProgramName, status: ToolStatus): string | null {
  switch (status.state) {
    case "found":
      return null;
    case "missing":
      return name === "wtsexporter"
        ? "wtsexporter hasn't been downloaded."
        : `${name} isn't on PATH and hasn't been downloaded.`;
    case "unavailable":
      return `The app has no ${name} download for this computer.`;
    case "downloadFailed":
      return `The ${name} download failed. ${status.reason}`;
    case "unusable":
      return `${name} can't be used. ${status.reason}`;
    case "downloading":
      return `${name} is downloading: ${downloadProgress(status.received, status.total)}.`;
  }
}

/** What the missing programs mean for the import, or for a program still downloading, that it waits. */
function consequence(need: ProgramNeed, blocked: boolean): string {
  if (need === "whatsapp") {
    return blocked ? "A WhatsApp import can't start without it." : "The import waits for it.";
  }
  return blocked
    ? "Convert and Compress need ffmpeg and ffprobe. Copy imports attachments as they are."
    : "Media waits for it.";
}

/**
 * Try again, which checks the Tools Directory and downloads what is missing
 * now rather than at the next start, and the troubleshooting section for
 * `need`.
 */
export function TryAgain({ need, offerRetry = true }: { need: ProgramNeed; offerRetry?: boolean }) {
  const retry = useRetryToolDownloads();
  // Try again started nothing because a check already runs (the start-up
  // check, say, still on another program): the status is asked for until
  // that check ends, and this says why nothing new started meanwhile.
  const checking = useToolsStatus().data?.checking ?? false;
  const alreadyRunning = retry.data === false && checking;
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
  programs: readonly ProgramName[];
  status: ToolsStatus;
  need: ProgramNeed;
}) {
  const lines = programs.flatMap((name) => {
    const line = programLine(name, status[name]);
    return line == null ? [] : [line];
  });
  if (lines.length === 0) return null;
  const unusable = programs.filter((name) => !toolUsable(status[name]));
  const blocked = unusable.length > 0;
  // A program the app has no download of for this computer can't be brought
  // by Try again: the troubleshooting section says where a copy goes.
  const retryable = unusable.some((name) => status[name].state !== "unavailable");
  return (
    <div role="status" className="mt-2 text-[0.813rem] text-muted">
      <p className="m-0">
        {lines.join(" ")} {consequence(need, blocked)}
      </p>
      {blocked ? <TryAgain need={need} offerRetry={retryable} /> : null}
    </div>
  );
}
