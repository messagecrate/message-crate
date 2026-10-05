/**
 * The Upload running now, as logging out sees it.
 *
 * The push of an Upload sends the session token it started with on every
 * request, so ending that session under it has every later request refused
 * and every remaining conversation recorded as failed (#1155). Logout pauses
 * the Upload first (CONTEXT.md, "Pause"), and asks before it does.
 *
 * The Import screen's run lives in `screens/import/useImportJob.ts`, which
 * registers the pause here while an Upload runs; `auth.tsx` reads it, without
 * depending on the screen.
 *
 * The other way round, a session the server refuses to the Import Run has to
 * end here, as one it refuses to a query does: to the push (#1491), and to the
 * run's own calls, such as a stage change or the completion (#1677).
 * `auth.tsx` registers how here, and the run calls it, without depending on
 * the login.
 */

let pauseUpload: (() => Promise<void>) | null = null;

/**
 * Record how to pause the Upload that has just started. `pause` stops the
 * push and resolves once the pause is recorded. Returns what to call once the
 * Upload has ended.
 */
export function registerRunningUpload(pause: () => Promise<void>): () => void {
  pauseUpload = pause;
  return () => {
    if (pauseUpload === pause) pauseUpload = null;
  };
}

/** True while an Upload runs. */
export function isUploadRunning(): boolean {
  return pauseUpload !== null;
}

/**
 * Pause the Upload that is running, and resolve once the push has halted and
 * the pause is recorded. Resolves at once when no Upload runs.
 */
export async function pauseRunningUpload(): Promise<void> {
  await pauseUpload?.();
}

let endSession: (token: string) => void = () => {};

/**
 * Record what ends the session when the server refuses the session token an
 * Import Run sent. Returns what to call once that no longer applies.
 */
export function onSessionRefused(handler: (token: string) => void): () => void {
  endSession = handler;
  return () => {
    if (endSession === handler) endSession = () => {};
  };
}

/**
 * The server refused `token`, the session the Import Run sent: end it here as
 * well, unless a later login has replaced it.
 */
export function sessionRefused(token: string): void {
  endSession(token);
}
