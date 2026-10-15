/**
 * The error a desktop job stopped by a Cancel rejects with, and the one a
 * run throws itself when a Cancel came first (`guard` in `runCancel.ts`).
 * An Import Run throws it too when the account that started it left
 * (`stopIfAccountLeft`), which pauses the run rather than canceling it: the
 * run handles both alike. Callers tell a Cancel from a failure with
 * `instanceof`, never by the error's text: the desktop app decides it from
 * the error's type (`canceled` on `DesktopJobErrorEvent`), so context added
 * to the error on its way out cannot turn a Cancel into a failure (#2077).
 */
export class CanceledError extends Error {
  constructor() {
    super("Canceled.");
    this.name = "CanceledError";
  }
}

/**
 * The error a desktop job that failed rejects with when the desktop app
 * sent a sentence for the person (`user_message`): the sentence is the
 * message, and `detail` is the error chain the sentence leaves out, for the
 * log and the run's record (#2071, #2112).
 */
export class DesktopJobError extends Error {
  readonly detail: string;

  constructor(message: string, detail: string) {
    super(message);
    this.name = "DesktopJobError";
    this.detail = detail;
  }
}
