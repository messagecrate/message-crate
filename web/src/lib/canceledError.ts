/**
 * The error a desktop job stopped by a Cancel rejects with, and the one a
 * run throws itself when a Cancel came first (`guard` in `runCancel.ts`).
 * An Import Run throws it too when the account that started it left
 * (`stopIfAccountLeft`), which is a stop that leaves the run resumable, not
 * a Cancel in `CONTEXT.md`'s sense: the run handles both alike. Callers tell a Cancel from a failure
 * with `instanceof`, never by the error's text: the desktop app decides it
 * from the error's type (`canceled` on `DesktopJobErrorEvent`), so context
 * added to the error on its way out cannot turn a Cancel into a failure
 * (#2077).
 */
export class CanceledError extends Error {
  constructor() {
    super("Canceled.");
    this.name = "CanceledError";
  }
}
