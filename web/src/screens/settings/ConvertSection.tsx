import { useCallback, useState } from "react";
import { ListBoxItem } from "react-aria-components";
import FormRow from "../../components/FormRow";
import PathPicker from "../../components/PathPicker";
import Select, { selectItemClassName } from "../../components/Select";
import TauriJobFormShell from "../../components/TauriJobFormShell";
import { useTauriJob } from "../../hooks/useTauriJob";
import { writeInExportDir } from "../../lib/exportDir";
import { parseSelectKey } from "../../lib/selectKey";
import { EXPORT_FORMATS, type ExportFormat, invokeFormat } from "../../lib/tauri";
import { isTauri } from "../../lib/tauri-check";
import { sameDirectory } from "./convertUtils";

const FORMAT_IDS = EXPORT_FORMATS.map((f) => f.id);

/** Label for the chosen format, for the success panel. */
function formatLabel(id: ExportFormat): string {
  return EXPORT_FORMATS.find((f) => f.id === id)?.label ?? id;
}

/**
 * Settings → Convert: rewrite a directory of already-exported files into another
 * format. `message-reexport` detects the input format from the directory, so the
 * screen picks the output format only, and it refuses to write into its own
 * input, so the two directories must differ. With no output directory chosen,
 * the conversion gets a directory of its own in the Export Directory
 * (`convert-2026-10-04-1430-csv`), deleted again if it fails.
 *
 * Convert reads files and writes files. It never opens a backup or the server,
 * which is why it lives under Settings as a tool rather than in the sidebar
 * beside Import and Export.
 */
export function ConvertSection() {
  const [inputDir, setInputDir] = useState("");
  const [outputDir, setOutputDir] = useState("");
  const [format, setFormat] = useState<ExportFormat>("jsonl");
  const [error, setError] = useState("");
  const [log, setLog] = useState<string[]>([]);
  // The directory and format each conversion was started with, so the success
  // message names what was written even after the form changes.
  const { running, finished, run, cancel } = useTauriJob<{
    outputDir: string;
    format: ExportFormat;
  }>({ job: "Convert" });

  const appendLog = useCallback((line: string) => {
    setLog((prev) => [...prev, line]);
  }, []);

  if (!isTauri()) {
    return (
      <p className="m-0 text-[0.875rem] text-muted">
        Convert rewrites a directory of exported files into another format. It is available in the
        desktop app.
      </p>
    );
  }

  const directoriesClash = sameDirectory(inputDir, outputDir);

  const startConvert = () => {
    if (running || directoriesClash) return;
    setError("");
    setLog([]);
    const chosen = outputDir.trim();
    const input = inputDir.trim();
    const convertInto = (output: string) =>
      run(
        () =>
          invokeFormat({
            input_dir: input,
            output_dir: output,
            output_format: format,
            started_from: "convert",
          }),
        { outputDir: output, format },
        { onLog: appendLog },
      );
    void (async () => {
      try {
        if (chosen) await convertInto(chosen);
        else
          await writeInExportDir(
            "convert",
            format,
            "",
            async (made) => {
              await convertInto(made.dir);
            },
            appendLog,
          );
      } catch (err: unknown) {
        const message = err instanceof Error ? err.message : String(err);
        appendLog(`Error: ${message}`);
        setError(message);
      }
    })();
  };

  return (
    <TauriJobFormShell
      className="max-w-[700px]"
      job="Convert"
      startLabel="Convert"
      runningLabel="Converting…"
      running={running}
      log={log}
      startDisabled={!inputDir.trim() || directoriesClash}
      onStart={startConvert}
      onCancel={cancel}
      error={error}
      intro={
        <p className="mb-6 text-[0.875rem] text-muted">
          Convert rewrites a directory of exported files into another format. The input format is
          read from the directory. With no output directory chosen, the result goes into a directory
          of its own in the Export Directory. Convert touches neither a backup nor your Message
          Crate.
        </p>
      }
      success={
        finished && !error ? (
          <div className="mt-4 rounded-md bg-ok-soft-bg p-4 text-[0.875rem]">
            Conversion complete. {formatLabel(finished.format)} written to {finished.outputDir}.
          </div>
        ) : null
      }
    >
      <FormRow label="Input directory">
        <PathPicker
          value={inputDir}
          onChange={setInputDir}
          directory
          placeholder="Directory holding an export…"
          isDisabled={running}
        />
      </FormRow>
      <FormRow label="Output directory">
        <PathPicker
          value={outputDir}
          onChange={setOutputDir}
          directory
          placeholder="The Export Directory"
          isDisabled={running}
        />
      </FormRow>
      {directoriesClash ? (
        <p role="alert" className="mb-3 ml-[calc(140px+0.75rem)] text-[0.813rem] text-danger">
          Choose a different output directory. Convert can't write into the directory it reads from,
          so the two directories must differ.
        </p>
      ) : null}
      <FormRow label="Output format">
        <Select
          selectedKey={format}
          onSelectionChange={(key) => {
            const next = parseSelectKey(key, FORMAT_IDS);
            if (next) setFormat(next);
          }}
          aria-label="Output format"
          isDisabled={running}
        >
          {EXPORT_FORMATS.map((option) => (
            <ListBoxItem key={option.id} id={option.id} className={selectItemClassName}>
              {option.label}
            </ListBoxItem>
          ))}
        </Select>
      </FormRow>
    </TauriJobFormShell>
  );
}
