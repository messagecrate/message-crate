import { errorText } from "../../../lib/apiErrorMessage";
import type { RunContext } from "./context";

/**
 * Delete a run directory of a run that has ended or been discarded. Never
 * throws: a refusal or failed delete is kept on `runDirDeleteFailure` for
 * the screen to show. Returns whether the directory is gone.
 */
export async function discardRunDirectory(ctx: RunContext, runDir: string): Promise<boolean> {
  const { store } = ctx;
  try {
    await ctx.desktop.invokeDeleteRunDir({ run_dir: runDir });
    store.set((state) =>
      state.runDirDeleteFailure?.path === runDir ? { runDirDeleteFailure: null } : {},
    );
    return true;
  } catch (e: unknown) {
    store.set({
      runDirDeleteFailure: {
        path: runDir,
        reason: errorText(e),
      },
    });
    return false;
  }
}

/** The person has read that a run directory was left behind. */
export function dismissRunDirDeleteFailure(ctx: RunContext): void {
  ctx.store.set({ runDirDeleteFailure: null });
}
