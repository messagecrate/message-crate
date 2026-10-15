import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  invokeCreateRunDir,
  invokeDeleteRunDir,
  invokeReadImportRunRecord,
  invokeSaveImportRunRecord,
} from "./runDir";

const invoke = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invoke(...args),
}));

describe("run directory command wrappers name only the directory", () => {
  const run = "/home/sam/message-crate/staging-run";

  beforeEach(() => {
    invoke.mockReset();
    invoke.mockResolvedValue(undefined);
  });

  // The desktop process keeps the Staging Directory and knows which directories
  // it made. A Staging Directory path sent from the window would be checked
  // against the Staging Directory the desktop process holds now, and a run
  // started under an earlier one would be refused (#1154).
  it("sends no Staging Directory with any run directory command", async () => {
    await invokeDeleteRunDir({ run_dir: run });
    await invokeReadImportRunRecord({ run_dir: run });
    await invokeSaveImportRunRecord({ run_dir: run, record: { issues: [] } });

    expect(invoke.mock.calls).toEqual([
      ["delete_run_dir", { args: { runDir: run } }],
      ["read_import_run_record", { args: { runDir: run } }],
      ["save_import_run_record", { args: { runDir: run, record: { issues: [] } } }],
    ]);
  });

  it("asks the desktop process to make a run's directory", async () => {
    invoke.mockResolvedValue(run);

    await expect(invokeCreateRunDir("imessage-ios")).resolves.toBe(run);

    expect(invoke).toHaveBeenCalledWith("create_run_dir", { label: "imessage-ios" });
  });
});
