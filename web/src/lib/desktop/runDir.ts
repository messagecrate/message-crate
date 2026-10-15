import { invoke } from "@tauri-apps/api/core";

/**
 * The staged directory `summarize_staging` and `transcode_staging` act on.
 *
 * It carries no media settings: `extract` recorded the run's in the directory,
 * and both commands read them from there, so they work to the values the
 * Import Run was started with.
 *
 * It carries no Staging Directory either. The desktop process keeps the
 * setting and the directories it made under it, and acts on a directory it made
 * wherever the setting points now, so changing the setting never strands a
 * run that started under the earlier one.
 */
export interface RunDirConfig {
  run_dir: string;
}

/**
 * Make a new run directory under the Staging Directory and return its path.
 * `label` is the Import source.
 */
export async function invokeCreateRunDir(label: string): Promise<string> {
  return invoke("create_run_dir", { label });
}

/**
 * Delete a run directory — the decline path's terminal action: closing an
 * review without approving deletes the directory outright.
 */
export async function invokeDeleteRunDir(config: { run_dir: string }): Promise<void> {
  return invoke("delete_run_dir", {
    args: { runDir: config.run_dir },
  });
}

/**
 * Read the Import Run record kept in a run directory
 * (`read_import_run_record`): what a paused run's earlier parts recorded.
 * Null when the directory holds none. The caller checks its shape.
 */
export async function invokeReadImportRunRecord(config: {
  run_dir: string;
}): Promise<unknown | null> {
  return invoke("read_import_run_record", {
    args: { runDir: config.run_dir },
  });
}

/** Write the Import Run record into its run directory (`save_import_run_record`). */
export async function invokeSaveImportRunRecord(config: {
  run_dir: string;
  record: unknown;
}): Promise<void> {
  return invoke("save_import_run_record", {
    args: { runDir: config.run_dir, record: config.record },
  });
}
