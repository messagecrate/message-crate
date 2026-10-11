import { getAccountId, getToken } from "../../../lib/api";
import { errorText } from "../../../lib/apiErrorMessage";
import { type ImportStage, setImportStage } from "../../../lib/importRun";
import { endsSession } from "../../../lib/routeQuery";
import { CANCELLED_MESSAGE } from "../../../lib/runCancel";
import { sessionRefused } from "../../../lib/sessionRefusal";
import type { StagingSummary } from "../../../lib/tauri";
import { importRunStore as store } from "../importRunStore";
import { discardRunDirectory } from "./runDirectory";
import { returnToForm } from "./steps";

/**
 * True when the account logged in now is not the one that started the run:
 * that account logged out while a stage ran. The server calls the run makes
 * go with the session logged in now, so the run must not make another.
 */
export function accountLeft(): boolean {
  return getAccountId() !== store.get().accountId;
}

/**
 * Stop the run where it got to, the way a Cancel does, once the account that
 * started it has left. The run stays open for that account to resume.
 */
export function stopIfAccountLeft(): void {
  if (accountLeft()) throw new Error(CANCELLED_MESSAGE);
}

/**
 * A stage change the server did not record. The run stops where the server
 * last recorded it, so a later visit resumes it from there.
 */
export class StageNotRecordedError extends Error {}

/**
 * A call the server refused the session to. `endSessionIfRefused` has
 * already ended the session here, so the run stops where the server has it,
 * for its account's next login to offer again. The run did nothing wrong, so
 * the refusal is no Import Error (#1677).
 */
export class SessionRefusedError extends Error {}

/**
 * Make one of the run's own server calls. These go through `serverApi.ts`
 * rather than TanStack Query, so the query client never sees their failures:
 * a `401 Unauthorized` that ends the session ends it here instead, as the
 * query client's would, and throws `SessionRefusedError`.
 *
 * The token is read before the call, because it is the one the call sends:
 * a refusal that arrives after a later login says nothing of that login's
 * session.
 */
export async function endSessionIfRefused<T>(call: () => Promise<T>): Promise<T> {
  const token = getToken();
  try {
    return await call();
  } catch (e: unknown) {
    if (token == null || !endsSession(e)) throw e;
    sessionRefused(token);
    throw new SessionRefusedError(errorText(e));
  }
}

/**
 * When `e` is a refused session, leave the run back on the form and return
 * true. The server keeps the run where it got to, with its run directory,
 * and the form's resume check offers it again at the account's next login. A
 * run the server never created has nothing to offer, so its directory goes.
 */
export async function leaveIfRefused(e: unknown, runId: number | null): Promise<boolean> {
  if (!(e instanceof SessionRefusedError)) return false;
  const { runDir } = store.get();
  if (runId == null && runDir != null) await discardRunDirectory(runDir);
  returnToForm();
  store.set({ running: false });
  return true;
}

/**
 * Move a live run to another stage, carrying the summary the person just
 * approved when there is one. `approvedPlan` is simply forwarded, undefined
 * and all: `setImportStage` posts `{ stage, summary: approvedPlan }`, and
 * `JSON.stringify` drops an `undefined`-valued property outright, so an
 * omitted plan and an explicit `undefined` reach the server identically —
 * no `summary` key at all, leaving whatever plan is already stored untouched.
 *
 * Throws `StageNotRecordedError` when the write fails, or
 * `SessionRefusedError` when the server refused the session. A later visit
 * resumes the run from the stage the server holds, so the caller must not go
 * on to work the server does not know the run reached.
 */
export async function moveStage(
  runId: number,
  stage: ImportStage,
  approvedPlan?: StagingSummary,
): Promise<void> {
  try {
    await endSessionIfRefused(() => setImportStage(runId, stage, approvedPlan));
  } catch (e: unknown) {
    if (e instanceof SessionRefusedError) throw e;
    const reason = errorText(e);
    throw new StageNotRecordedError(`Message Crate didn't record the run's progress: ${reason}`);
  }
}

/**
 * Record that the run is waiting at a review. Returns `recorded` when the
 * server has it. A failure returns `failed` and leaves the review on screen
 * with the error on it (`reviewError`), and approving writes the stage again
 * before anything else (`approve`). A refused session is no failure of the
 * run: the run leaves for the form (`leaveIfRefused`), and this returns
 * `refused`, for the caller to stop.
 */
export async function moveStageAtReview(
  runId: number,
  stage: "staging_review" | "media_review",
  approvedPlan?: StagingSummary,
): Promise<"recorded" | "failed" | "refused"> {
  try {
    await moveStage(runId, stage, approvedPlan);
    store.set({ reviewError: null });
    return "recorded";
  } catch (e: unknown) {
    if (await leaveIfRefused(e, runId)) return "refused";
    store.set({ reviewError: errorText(e) });
    return "failed";
  }
}
