/**
 * A promise and the functions that settle it, for a test to decide when a
 * fake answers, or for a fake to tell the test it was reached. A test awaits
 * the promise inside `act` rather than polling the screen with `waitFor` or
 * `findBy`, which wait against a timeout.
 */
export function deferred<T = void>(): {
  promise: Promise<T>;
  resolve: (value: T) => void;
  reject: (reason?: unknown) => void;
} {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((settle, fail) => {
    resolve = settle;
    reject = fail;
  });
  return { promise, resolve, reject };
}
