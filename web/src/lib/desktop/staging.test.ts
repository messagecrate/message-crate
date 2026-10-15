import { beforeEach, describe, expect, it, vi } from "vitest";
import { invokeSummarizeStaging, invokeTranscodeStaging } from "./staging";

const invoke = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invoke(...args),
}));

describe("staging command wrappers name only the directory", () => {
  const run = "/home/sam/message-crate/staging-run";

  beforeEach(() => {
    invoke.mockReset();
    invoke.mockResolvedValue(undefined);
  });

  // The desktop process keeps the Staging Directory and knows which directories
  // it made. A Staging Directory path sent from the window would be checked
  // against the Staging Directory the desktop process holds now, and a run
  // started under an earlier one would be refused (#1154).
  it("sends no Staging Directory with any staging command", async () => {
    await invokeSummarizeStaging({ run_dir: run });
    await invokeTranscodeStaging({ run_dir: run });

    expect(invoke.mock.calls).toEqual([
      ["summarize_staging", { args: { runDir: run } }],
      ["transcode_staging", { args: { runDir: run } }],
    ]);
  });
});
