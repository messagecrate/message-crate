import { invoke } from "@tauri-apps/api/core";
import type { components } from "../serverApi.types";

/**
 * The list an export's search is for: the messages it matches on the
 * Messages list, or every message of the conversations it shows on the
 * Conversations list. The server's own type, so the two cannot drift.
 */
export type ExportQueryList = components["schemas"]["ExportQueryList"];

interface ExportConfig {
  base_url: string;
  token: string;
  out_dir: string;
  /** Blank exports everything the account holds. */
  query: string;
  /** The list `query` is for. Unused when `query` is blank. */
  list: ExportQueryList;
  skip_attachments: boolean;
}

/** Download conversations from a server into a directory. */
export async function invokeExport(config: ExportConfig): Promise<void> {
  return invoke("export", {
    args: {
      baseUrl: config.base_url,
      token: config.token,
      outDir: config.out_dir,
      query: config.query,
      list: config.list,
      skipAttachments: config.skip_attachments,
    },
  });
}

/**
 * File formats `message-reexport` can write, in the order the Export screen
 * offers them. The ids are the strings the `format` command parses
 * (`src-tauri/src/commands/format.rs`); anything else is rejected there.
 */
export const EXPORT_FORMATS = [
  { id: "jsonl", label: "JSON Lines (.jsonl)" },
  { id: "json", label: "JSON (.json)" },
  { id: "csv", label: "CSV (.csv)" },
  { id: "eml", label: "EML (one file per message)" },
  { id: "mbox", label: "MBOX (.mbox)" },
  { id: "xml", label: "Android XML (smses.xml)" },
  { id: "sms-backup-plus", label: "EML (SMS Backup+)" },
] as const;

/** Id of a format the Export screen can write. */
export type ExportFormat = (typeof EXPORT_FORMATS)[number]["id"];

/**
 * The directory an Export or Convert gets of its own in the Export Directory,
 * and where an Export keeps its in-between files inside it.
 */
export interface ExportDir {
  /** Where the result lands unless another destination is chosen. */
  dir: string;
  /** Where an Export writes its JSON Lines before converting them. */
  exported: string;
  /** Where an Export's conversion writes when the result lands in `dir`. */
  converting: string;
}

/** The Export Directory, for Settings. */
export async function invokeExportDirectory(): Promise<string> {
  return invoke("export_directory");
}

/**
 * Make the directory of a new Export or Convert in the Export Directory.
 * `chosen` is the destination the person chose, or empty; one that holds the
 * Export Directory is refused before anything is made.
 */
export async function invokeCreateExportDir(
  kind: "export" | "convert",
  format: ExportFormat,
  chosen: string,
): Promise<ExportDir> {
  return invoke("create_export_dir", { kind, format, chosen: chosen || null });
}

/**
 * Finish an Export's or Convert's directory after it succeeded: its
 * in-between files are deleted and only the result is left. Returns the
 * directory, or null when the result went elsewhere and the directory was
 * deleted.
 */
export async function invokeFinishExportDir(dir: string): Promise<string | null> {
  return invoke("finish_export_dir", { dir });
}

/** Delete an Export's or Convert's directory after it failed or was cancelled. */
export async function invokeDiscardExportDir(dir: string): Promise<void> {
  return invoke("discard_export_dir", { dir });
}

/**
 * Rewrite an export directory into another format.
 *
 * `input_dir` and `output_dir` must differ: `message-reexport` canonicalizes
 * both and refuses to write into its own input.
 */
export async function invokeFormat(config: {
  input_dir: string;
  output_dir: string;
  output_format: ExportFormat;
  /** The screen that started the run, which the desktop names it by when it
   * refuses another one while this runs. */
  started_from: "export" | "convert";
  /** When the Export Run started, in epoch milliseconds: SMS Backup+ mail
   * records it as its backup time. Left out by Settings → Convert, which is a
   * run of its own. */
  run_started_ms?: number;
}): Promise<void> {
  return invoke("format", {
    inputDir: config.input_dir,
    outputDir: config.output_dir,
    outputFormat: config.output_format,
    startedFrom: config.started_from,
    runStartedMs: config.run_started_ms ?? null,
  });
}
