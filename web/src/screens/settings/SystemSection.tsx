import { type ReactNode, useEffect, useId, useRef, useState } from "react";
import Checkbox from "../../components/Checkbox";
import { CheckIcon, DownloadIcon, XIcon } from "../../components/icons";
import OpenPathButton from "../../components/OpenPathButton";
import PathPicker from "../../components/PathPicker";
import PlainButton from "../../components/PlainButton";
import { getBaseUrl } from "../../lib/api";
import { errorText } from "../../lib/apiErrorMessage";
import { APP_BUILD } from "../../lib/build";
import { getOpenToNetwork, isOwnAddress, setOpenToNetwork } from "../../lib/localServer";
import { getRememberImporterPaths, setRememberImporterPaths } from "../../lib/system-settings";
import {
  invokeExportDirectory,
  invokeOpenDataDirectory,
  invokeSetOpenToNetwork,
  invokeSetStagingRoot,
  invokeStagingRoot,
  type LocalServerStatus,
  type ToolName,
  type ToolStatus,
  type ToolsStatus,
} from "../../lib/tauri";
import { isTauri } from "../../lib/tauri-check";
import { readerLicenseUrl, readerSourceUrl } from "../../lib/thirdPartySoftware";
import { toolStatusLine, troubleshootingSentence } from "../../lib/toolStatusCopy";
import { accentLinkClass } from "../../lib/uiStyles";
import { useToolsStatus } from "../../lib/useToolsStatus";

const sectionHeadingClass =
  "m-0 mb-2 text-[12px] font-semibold uppercase tracking-[0.05em] text-muted";

const EXAMPLE_STAGING = "staging-iphone-ios-260809-143022";

/**
 * Shared label + control grid so the path fields share one nowrap label column.
 * Below the `sm` width the label sits above its control, so a phone-width
 * window does not scroll sideways.
 */
const settingsGridClass =
  "grid grid-cols-[minmax(0,1fr)] items-center gap-x-3 gap-y-1 sm:grid-cols-[13.5rem_minmax(0,1fr)]";
const settingsLabelClass = "whitespace-nowrap text-[0.875rem] font-medium text-text";
/** A line under a control, kept in the control's column from `sm`. */
const settingsNoteLayoutClass = "pl-2 text-[0.75rem] sm:col-start-2";
const settingsHelpClass = `${settingsNoteLayoutClass} text-muted`;

/** Example path shown under the Staging directory field. */
function stagingHelpExample(stagingDir: string, defaultDir: string): string {
  const trimmed = stagingDir.trim().replace(/[/\\]+$/, "");
  const defaultTrimmed = defaultDir.trim().replace(/[/\\]+$/, "");
  if (!trimmed || (defaultTrimmed && trimmed === defaultTrimmed)) {
    return `~/message-crate/${EXAMPLE_STAGING}`;
  }
  return `${trimmed}/${EXAMPLE_STAGING}`;
}

/** Said once on every failed download's line, whatever the reason: each is retried. */
const DOWNLOAD_RETRIED =
  "It is tried again the next time the app starts, or with Try again on the Import form.";

/**
 * Where to put a program that is missing, as the words before the Tools
 * Directory's path (`dir`), or the whole sentence when `dir` is null.
 * ffmpeg and ffprobe are used only from one place, so a missing one goes
 * beside the one that was found, or both go in the Tools Directory.
 * wtsexporter is looked for in the Tools Directory only.
 */
function missingFix(
  name: ToolName,
  partner: ToolStatus | null,
  toolsDir: string | null,
): { text: string; dir: string | null } | null {
  if (partner == null) {
    return toolsDir ? { text: "Put it in the Tools Directory", dir: toolsDir } : null;
  }
  const other = name === "ffmpeg" ? "ffprobe" : "ffmpeg";
  if (partner.state === "found") {
    return toolsDir
      ? { text: `Put it beside ${other}, or put both in the Tools Directory`, dir: toolsDir }
      : { text: `Put it beside ${other}`, dir: null };
  }
  return toolsDir ? { text: `Put it and ${other} in the Tools Directory`, dir: toolsDir } : null;
}

/**
 * What Settings says after a program's status line: where a missing one
 * goes, and for a failed download that it is tried again and the
 * troubleshooting section. As text for the row's label, and as the row shows it.
 */
function statusExtra(
  name: ToolName,
  status: ToolStatus,
  partner: ToolStatus | null,
  toolsDir: string | null,
): { label: string; node: ReactNode } | null {
  const fixWithDir = (fix: { text: string; dir: string | null } | null) =>
    fix == null
      ? null
      : {
          label: fix.dir ? `${fix.text}, ${fix.dir}.` : `${fix.text}.`,
          node: (
            <>
              {fix.text}
              {fix.dir ? (
                <>
                  , <code className="break-all font-mono text-[0.7rem]">{fix.dir}</code>
                </>
              ) : null}
              .
            </>
          ),
        };
  switch (status.state) {
    // An app opened from the Dock on macOS doesn't see the PATH a shell
    // sets, so a program installed with Homebrew is missing here until it
    // is linked into the Tools Directory.
    case "missing":
      return fixWithDir(missingFix(name, partner, toolsDir));
    // No download for this computer: only a copy put there by hand is used.
    case "unavailable":
      return fixWithDir(
        toolsDir ? { text: "Put a copy in the Tools Directory", dir: toolsDir } : null,
      );
    case "downloadFailed": {
      const { before, section, after } = troubleshootingSentence(name);
      return {
        label: `${DOWNLOAD_RETRIED} ${before}${section.heading}${after}`,
        node: (
          <>
            {DOWNLOAD_RETRIED} {before}
            <a href={section.url} target="_blank" rel="noopener" className={accentLinkClass}>
              {section.heading}
            </a>
            {after}
          </>
        ),
      };
    }
    default:
      return null;
  }
}

/**
 * One program's status line: the path it was found at, or the line the
 * Import form shows too (`toolStatusLine`), with what Settings adds after it.
 */
function ToolStatusRow({
  name,
  status,
  partner,
  toolsDir,
}: {
  name: ToolName;
  status: ToolStatus;
  /** For ffmpeg the status of ffprobe, and the reverse; null for wtsexporter. */
  partner: ToolStatus | null;
  toolsDir: string | null;
}) {
  if (status.state === "found") {
    return (
      <li
        className="flex items-start gap-1.5 text-[0.75rem] text-text"
        aria-label={`Found ${name} - ${status.path}`}
      >
        <CheckIcon size={14} className="mt-0.5 shrink-0 text-ok" />
        <span>
          Found <code className="font-mono text-[0.7rem]">{name}</code>
          {" - "}
          <code className="break-all font-mono text-[0.7rem]">{status.path}</code>
        </span>
      </li>
    );
  }
  const line = toolStatusLine(status);
  const extra = statusExtra(name, status, partner, toolsDir);
  const label = `${line.before}${name}${line.after}${extra ? ` ${extra.label}` : ""}`;
  return (
    <li className="flex items-start gap-1.5 text-[0.75rem] text-text" aria-label={label}>
      {status.state === "downloading" ? (
        <DownloadIcon size={14} className="mt-0.5 shrink-0 text-muted" />
      ) : (
        <XIcon size={14} className="mt-0.5 shrink-0 text-danger" />
      )}
      <span>
        {line.before}
        <code className="font-mono text-[0.7rem]">{name}</code>
        {line.after}
        {extra ? <> {extra.node}</> : null}
      </span>
    </li>
  );
}

/**
 * Where ffmpeg, ffprobe and wtsexporter are, read from the desktop process.
 * Nothing here is typed: ffmpeg and ffprobe come from PATH, then the Tools
 * Directory, and wtsexporter only from the Tools Directory.
 */
function MediaTools({ tools, error }: { tools: ToolsStatus | null; error: string | null }) {
  return (
    <div className="mt-8">
      <h3 className={sectionHeadingClass}>Media</h3>
      <p className="m-0 max-w-prose text-[0.813rem] text-muted">
        ffmpeg and ffprobe are looked for on PATH, then in the Tools Directory, and both must come
        from one of them. wtsexporter is looked for in the Tools Directory only.
      </p>
      {tools?.toolsDir ? (
        <div className={`${settingsGridClass} mt-2`}>
          <span className={settingsLabelClass}>Tools directory</span>
          <span className="break-all pl-2 font-mono text-[0.813rem] text-text">
            {tools.toolsDir}
          </span>
        </div>
      ) : null}
      {error ? (
        <p className="m-0 mt-2 text-[0.75rem] text-danger" role="alert">
          {error}
        </p>
      ) : null}
      {tools ? (
        <ul className="m-0 mt-2 list-none space-y-1 p-0" aria-label="Media tools">
          <ToolStatusRow
            name="ffmpeg"
            status={tools.ffmpeg}
            partner={tools.ffprobe}
            toolsDir={tools.toolsDir}
          />
          <ToolStatusRow
            name="ffprobe"
            status={tools.ffprobe}
            partner={tools.ffmpeg}
            toolsDir={tools.toolsDir}
          />
          <ToolStatusRow
            name="wtsexporter"
            status={tools.wtsexporter}
            partner={null}
            toolsDir={tools.toolsDir}
          />
        </ul>
      ) : null}
    </div>
  );
}

/** This app's own Build, shown on the System tab in the browser and the desktop app alike. */
function AppVersion() {
  return (
    <div>
      <h3 className={sectionHeadingClass}>About</h3>
      <div className={settingsGridClass}>
        <span className={settingsLabelClass}>Version</span>
        <span className="break-all pl-2 font-mono text-[0.813rem] text-text">{APP_BUILD}</span>
      </div>
    </div>
  );
}

/**
 * The notice the GPL asks for. The desktop installer ships the Apple Messages
 * reader, a separate program under the GNU GPL v3, so whoever installed the
 * desktop app received a copy and must be able to find its license and the
 * source that matches it. The website ships no such program, so the browser
 * build never shows this.
 */
function ThirdPartySoftware() {
  const linkClass = "text-accent";
  return (
    <div className="mt-8">
      <h3 className={sectionHeadingClass}>Third-party software</h3>
      <p className="m-0 max-w-prose text-[0.875rem] text-text">
        Apple Messages are read by the Apple Messages reader (imessage-reader), a separate program
        installed beside this app. It is free software under the GNU General Public License, version
        3 or later, and its source is published with each release.
      </p>
      <p className="m-0 mt-1 text-[0.875rem]">
        <a href={readerSourceUrl(APP_BUILD)} target="_blank" rel="noopener" className={linkClass}>
          Source
        </a>
        <span className="text-muted"> · </span>
        <a href={readerLicenseUrl(APP_BUILD)} target="_blank" rel="noopener" className={linkClass}>
          License
        </a>
      </p>
    </div>
  );
}

/**
 * Where the Message Crate this app starts keeps everything. The directory is the
 * whole Message Crate, so it is what a person copies to back it up; the app
 * opens it rather than naming a path to find.
 */
function DataDirectory() {
  const [error, setError] = useState<string | null>(null);
  const [open, setOpen] = useState(getOpenToNetwork);
  const [server, setServer] = useState<LocalServerStatus | null>(null);
  const [networkError, setNetworkError] = useState<string | null>(null);

  const onOpenChange = (on: boolean) => {
    setOpen(on);
    setOpenToNetwork(on);
    setNetworkError(null);
    // While the app uses another Message Crate, the setting waits for the
    // next start of its own.
    if (!isOwnAddress(getBaseUrl())) return;
    invokeSetOpenToNetwork(on).then(setServer, (caught: unknown) => {
      setNetworkError(errorText(caught));
    });
  };
  // A Message Crate the app only found (Docker on this computer) listens
  // where it was told to, not where this setting says.
  const notOurs = server?.status === "ready" && !server.started_by_app;
  return (
    <div className="mt-8">
      <h3 className={sectionHeadingClass}>Message Crate on this computer</h3>
      <p className="m-0 max-w-prose text-[0.875rem] text-text">
        The Message Crate this app starts keeps its database and attachments in one directory. A
        copy of that directory is a backup.
      </p>
      <PlainButton
        className="mt-2 rounded border border-border px-3 py-1.5 text-[0.875rem] text-text hover:bg-elevated"
        onPress={() => {
          setError(null);
          invokeOpenDataDirectory().catch((caught: unknown) => {
            setError(errorText(caught));
          });
        }}
      >
        Open data directory
      </PlainButton>
      {error ? (
        <p className="m-0 mt-1 text-[0.75rem] text-danger" role="alert">
          {error}
        </p>
      ) : null}

      <Checkbox
        labelClassName="mt-5 flex items-start text-[0.875rem]"
        className="mt-[0.15rem]"
        checked={open}
        onChange={onOpenChange}
      >
        <span>
          Let other devices on this network connect
          <span className="mt-1 block max-w-prose text-[0.75rem] text-muted">
            A phone or another computer can then open this computer's address on port 8080, for as
            long as this app is open. The connection is plain HTTP, so anyone on the network can
            read what is sent, passwords included. Changing this restarts Message Crate once no
            import is running.
          </span>
        </span>
      </Checkbox>
      {notOurs ? (
        <p className="m-0 mt-1 max-w-prose text-[0.75rem] text-muted" role="status">
          This app did not start the Message Crate that is running on this computer, so the setting
          does not change it.
        </p>
      ) : null}
      {networkError ? (
        <p className="m-0 mt-1 text-[0.75rem] text-danger" role="alert">
          {networkError}
        </p>
      ) : null}
    </div>
  );
}

/**
 * Where each Export and Convert gets a directory of its own, unless the
 * person chooses another one on its screen. The desktop process keeps it in
 * the app-data directory; the button opens it.
 */
function ExportDirectory() {
  const [path, setPath] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    let live = true;
    invokeExportDirectory().then(
      (dir) => {
        if (live) setPath(dir);
      },
      (caught: unknown) => {
        if (live) setError(errorText(caught));
      },
    );
    return () => {
      live = false;
    };
  }, []);
  return (
    <div className="mt-8">
      <h3 className={sectionHeadingClass}>Exports</h3>
      <p className="m-0 max-w-prose text-[0.875rem] text-text">
        Each Export and Convert gets a directory of its own in the Export Directory, unless you
        choose another directory to save to.
      </p>
      {path ? (
        <OpenPathButton
          path={path}
          className="mt-2 max-w-full border-0 bg-transparent p-0 text-left text-[0.875rem] text-accent underline-offset-2 [overflow-wrap:anywhere] hover:underline"
        >
          {path}
        </OpenPathButton>
      ) : null}
      {error ? (
        <p className="m-0 mt-1 text-[0.75rem] text-danger" role="alert">
          {error}
        </p>
      ) : null}
    </div>
  );
}

export function SystemSection() {
  const stagingId = useId();
  const [stagingPath, setStagingPath] = useState("");
  const [defaultStagingPath, setDefaultStagingPath] = useState("");
  /** The Staging Directory the desktop process holds now. */
  const [stagingError, setStagingError] = useState<string | null>(null);
  const [rememberPaths, setRememberPaths] = useState(false);
  // Asked again each second while a download runs.
  const toolsQuery = useToolsStatus();
  const tools: ToolsStatus | null = toolsQuery.data ?? null;
  const toolsError = toolsQuery.error
    ? `Could not look for the media tools. ${errorText(toolsQuery.error)}`
    : null;

  useEffect(() => {
    if (!isTauri()) return;

    setRememberPaths(getRememberImporterPaths());

    void (async () => {
      try {
        const staging = await invokeStagingRoot();
        setDefaultStagingPath(staging.defaultRoot);
        setStagingPath(staging.root);
      } catch (caught: unknown) {
        setStagingError(errorText(caught));
      }
    })();
  }, []);

  // The desktop process keeps the setting and decides what it takes: every
  // value typed is sent, and a refusal is shown as the desktop process gave
  // it, so the window holds no rule of its own. An empty value goes back to
  // the default. A run already staged keeps the directory it was made in; the
  // new setting applies to runs started after it.
  const stagingSaveGen = useRef(0);
  const lastStagingSave = useRef<Promise<unknown>>(Promise.resolve());
  const onStagingPathChange = (next: string) => {
    setStagingPath(next);
    const gen = ++stagingSaveGen.current;
    const save = invokeSetStagingRoot(next.trim());
    lastStagingSave.current = save;
    save.then(
      () => {
        if (gen !== stagingSaveGen.current) return;
        setStagingError(null);
      },
      (caught: unknown) => {
        if (gen !== stagingSaveGen.current) return;
        setStagingError(`Not saved. ${errorText(caught)}`);
      },
    );
  };

  // A value the desktop process refused stays in the field until it is left.
  // The field then shows the directory the desktop process holds, read once the
  // last save has answered, so a value accepted after the refusal is never
  // hidden behind the one shown before it.
  const onStagingPathBlur = () => {
    if (stagingError === null) return;
    const gen = ++stagingSaveGen.current;
    setStagingError(null);
    lastStagingSave.current
      .catch(() => undefined)
      .then(() => invokeStagingRoot())
      .then(
        (staging) => {
          if (gen !== stagingSaveGen.current) return;
          setStagingPath(staging.root);
        },
        (caught: unknown) => {
          if (gen !== stagingSaveGen.current) return;
          setStagingError(errorText(caught));
        },
      );
  };

  if (!isTauri()) {
    return (
      <div>
        <AppVersion />
        <p className="m-0 mt-8 text-[0.875rem] text-muted">
          System settings (the Staging Directory, remembered importer paths, the media tools, and
          the Export Directory) are available in the desktop app.
        </p>
      </div>
    );
  }

  const helpExample = stagingHelpExample(stagingPath, defaultStagingPath);

  return (
    <div>
      <h3 className={sectionHeadingClass}>Staging</h3>
      <div className={settingsGridClass}>
        <label htmlFor={stagingId} className={settingsLabelClass}>
          Staging directory
        </label>
        <div>
          <PathPicker
            id={stagingId}
            value={stagingPath}
            onChange={onStagingPathChange}
            onBlur={onStagingPathBlur}
            directory
            placeholder={defaultStagingPath || "~/message-crate"}
          />
        </div>
        {stagingError ? (
          <p className={`${settingsNoteLayoutClass} m-0 text-danger`} role="alert">
            {stagingError}
          </p>
        ) : null}
        <p className={settingsHelpClass}>
          Each Import Run gets a directory here while it runs. For example {helpExample}
        </p>
      </div>

      <Checkbox
        labelClassName="mt-5 flex items-start text-[0.875rem]"
        className="mt-[0.15rem]"
        checked={rememberPaths}
        onChange={(on) => {
          setRememberPaths(on);
          setRememberImporterPaths(on);
        }}
      >
        <span>
          Remember importer paths
          <span className="mt-1 block text-[0.75rem] text-muted">
            When enabled, Import restores the last backup path for each import source.
          </span>
        </span>
      </Checkbox>

      <MediaTools tools={tools} error={toolsError} />

      <ExportDirectory />

      <DataDirectory />

      <div className="mt-8">
        <AppVersion />
        <ThirdPartySoftware />
      </div>
    </div>
  );
}
