import { keepPreviousData } from "@tanstack/react-query";
import { useEffect, useRef, useState } from "react";
import { useRouteInfiniteQuery } from "../../lib/routeQuery";
import { saveFile } from "../../lib/saveFile";
import type { LogLevel, LogLine, LogLinesPage } from "../../lib/serverApi";
import Button from "../Button";
import Select, { ListBoxItem, selectItemClassName } from "../Select";
import TextField from "../TextField";
import { type LevelFilter, type LogDownload, type LogSource, logErrorMessage } from "./logSource";

/** Lines read per page. */
export const LOG_PAGE_SIZE = 200;

/** How long the search waits after the last key before it reads the log. */
const SEARCH_DELAY_MS = 250;

const LEVEL_LABELS: Record<LevelFilter, string> = {
  error: "Errors",
  warn: "Warnings and up",
  all: "Everything",
};

/** The level a filter asks the log for; every line when none. */
function levelParam(filter: LevelFilter): LogLevel | undefined {
  return filter === "all" ? undefined : filter;
}

const LEVEL_CLASS: Record<LogLevel, string> = {
  error: "text-danger",
  warn: "text-warn-soft-text",
  info: "text-muted",
  debug: "text-muted",
  trace: "text-muted",
};

const itemClassName = (state: { isFocused: boolean; isSelected: boolean }) =>
  selectItemClassName(state, "sm");

/**
 * One log, read: a level filter (opens at warnings and up), a search that
 * reads as it is typed, a download of the log as it is, and the lines newest
 * first, older ones read as the list scrolls near its end. Nothing reads new
 * lines on its own: the list shows the log as it was when it was read.
 *
 * Owner Home's Logs panel shows the server's log and the Import Run logs on
 * this computer through it, and an Import Run's row in Settings → Storage
 * shows that run's log, so every log reads the same way.
 */
export default function LogViewer({
  source,
  downloads,
}: {
  source: LogSource;
  /** The files the log downloads as. */
  downloads: LogDownload[];
}) {
  const [level, setLevel] = useState<LevelFilter>("warn");
  const [typed, setTyped] = useState("");
  const [text, setText] = useState("");
  useEffect(() => {
    const timer = setTimeout(() => setText(typed.trim()), SEARCH_DELAY_MS);
    return () => clearTimeout(timer);
  }, [typed]);

  const lines = useRouteInfiniteQuery<LogLinesPage, number | undefined>(
    [...source.key, "lines", level, text],
    {
      queryFn: ({ pageParam, signal }) =>
        source.readLines(
          {
            level: levelParam(level),
            text: text || undefined,
            after: pageParam,
            limit: LOG_PAGE_SIZE,
          },
          signal,
        ),
      initialPageParam: undefined,
      getNextPageParam: (last) => (last.has_more ? last.items.at(-1)?.id : undefined),
      placeholderData: keepPreviousData,
      // A log only grows at its newest end, and the list shows it as read.
      refetchOnWindowFocus: false,
    },
  );
  const items = lines.data?.pages.flatMap((page) => page.items) ?? [];

  const scroller = useRef<HTMLDivElement>(null);
  const end = useRef<HTMLDivElement>(null);
  const { hasNextPage, isFetchingNextPage, fetchNextPage } = lines;
  useEffect(() => {
    const target = end.current;
    if (!target || !hasNextPage) return;
    const observer = new IntersectionObserver(
      (entries) => {
        if (entries.some((entry) => entry.isIntersecting) && !isFetchingNextPage) {
          void fetchNextPage();
        }
      },
      { root: scroller.current, rootMargin: "200px" },
    );
    observer.observe(target);
    return () => observer.disconnect();
  }, [hasNextPage, isFetchingNextPage, fetchNextPage]);

  return (
    <div className="flex min-h-0 flex-col gap-3">
      <div className="flex flex-wrap items-end gap-2">
        <Select
          aria-label="Show"
          size="sm"
          className="w-[11rem]"
          selectedKey={level}
          onSelectionChange={(key) => {
            if (key != null) setLevel(String(key) as LevelFilter);
          }}
        >
          {(Object.keys(LEVEL_LABELS) as LevelFilter[]).map((id) => (
            <ListBoxItem key={id} id={id} className={itemClassName}>
              {LEVEL_LABELS[id]}
            </ListBoxItem>
          ))}
        </Select>
        <TextField
          aria-label="Search the log"
          placeholder="Search the log"
          className="min-w-[12rem] flex-1"
          inputClassName="!py-1 text-[0.813rem]"
          value={typed}
          onChange={setTyped}
        />
        <LogDownloads downloads={downloads} />
      </div>
      {lines.isPending ? (
        <p className="m-0 text-[0.813rem] text-muted">Reading the log…</p>
      ) : lines.error && items.length === 0 ? (
        <p role="alert" className="m-0 text-[0.813rem] text-danger">
          {logErrorMessage(lines.error, "Could not read the log.")}
        </p>
      ) : items.length === 0 ? (
        <p className="m-0 text-[0.813rem] text-muted">
          {text ? "No line matches the search." : `No line at this level.`}
        </p>
      ) : (
        <div
          ref={scroller}
          className="max-h-[70vh] overflow-auto rounded-md border border-border bg-panel"
        >
          <ol aria-label="Log lines" className="m-0 list-none p-0 font-mono text-[0.75rem]">
            {items.map((line) => (
              <LogLineRow key={line.id} line={line} />
            ))}
          </ol>
          <div ref={end} aria-hidden="true" className="h-px" />
          {isFetchingNextPage ? (
            <p className="m-0 px-2 py-1 text-[0.75rem] text-muted">Reading older lines…</p>
          ) : null}
        </div>
      )}
    </div>
  );
}

/** One line: its time, its level and what it says. */
function LogLineRow({ line }: { line: LogLine }) {
  return (
    <li className="flex gap-2 border-b border-border px-2 py-0.5 last:border-b-0">
      <span className="shrink-0 text-muted">{line.time}</span>
      <span className={`w-12 shrink-0 uppercase ${LEVEL_CLASS[line.level]}`}>{line.level}</span>
      <span className="min-w-0 whitespace-pre-wrap break-all text-text">{line.text}</span>
    </li>
  );
}

/** One button per file the log downloads as, each saving the file as it is. */
function LogDownloads({ downloads }: { downloads: LogDownload[] }) {
  const [busy, setBusy] = useState("");
  const [error, setError] = useState("");
  const save = async (download: LogDownload) => {
    setBusy(download.name);
    setError("");
    try {
      const contents = await download.read();
      await saveFile(download.name, new Blob([contents], { type: "text/plain" }));
    } catch (err) {
      setError(logErrorMessage(err, `Could not download ${download.name}.`));
    } finally {
      setBusy("");
    }
  };
  return (
    <span className="flex flex-wrap items-center gap-2">
      {downloads.map((download) => (
        <Button
          key={download.name}
          type="button"
          variant="ghost"
          size="chip"
          disabled={busy !== ""}
          title={`Download ${download.name} as it is`}
          onClick={() => void save(download)}
        >
          {downloads.length === 1 ? "Download" : `Download ${download.name}`}
        </Button>
      ))}
      {error ? (
        <span role="alert" className="text-[0.75rem] text-danger">
          {error}
        </span>
      ) : null}
    </span>
  );
}
