import { type ComponentType, type LazyExoticComponent, lazy } from "react";
import { errorText } from "./apiErrorMessage";
import { pageAnswers, reloadPage } from "./pageReload";

/**
 * The session storage entry that says this tab already reloaded once for a
 * screen that didn't load, so a screen that fails again shows why instead of
 * reloading forever.
 */
const RELOADED_FOR_SCREEN = "mc-reloaded-for-screen";

/**
 * A screen whose code didn't arrive. Its usual cause is a new release: the
 * server replaced the app's files, and a tab opened before the upgrade asks
 * for a file by a name the server no longer has. The browser's own error
 * (`cause`) is the message.
 */
export class ScreenLoadError extends Error {
  constructor(cause: unknown) {
    super(errorText(cause), { cause });
    this.name = "ScreenLoadError";
  }
}

function forgetReload(): void {
  try {
    sessionStorage.removeItem(RELOADED_FOR_SCREEN);
  } catch {
    // Storage the browser refuses holds no mark to forget.
  }
}

/**
 * `lazy`, for a screen, or a panel such as Advanced search, whose code is
 * downloaded when it is first shown. A
 * download that fails rejects with `ScreenLoadError`, which `ErrorBoundary`
 * answers by reloading the page once (`reloadOnceForScreen`). A download that
 * arrives clears the mark that reload left, so a later release reloads again.
 */
export function lazyScreen<P>(
  load: () => Promise<{ default: ComponentType<P> }>,
): LazyExoticComponent<ComponentType<P>> {
  return lazy(() =>
    load().then(
      (screen) => {
        forgetReload();
        return screen;
      },
      (e: unknown) => {
        throw new ScreenLoadError(e);
      },
    ),
  );
}

function alreadyReloaded(): boolean {
  try {
    return sessionStorage.getItem(RELOADED_FOR_SCREEN) !== null;
  } catch {
    // Nothing would stop the next failure from reloading again.
    return true;
  }
}

/**
 * Reload the page for a screen that didn't load, unless this tab already did
 * since a screen last loaded, or the page itself no longer answers. A reload
 * fetches the current release's files, which is what a screen from an earlier
 * release needs. A page that doesn't answer, such as the website while its
 * server is down, would reload into the browser's own error page, so the
 * person keeps the app and its message instead. A browser that refuses
 * session storage gets no reload either, because nothing would stop the next
 * failure from reloading again. Resolves to whether it reloaded.
 */
export async function reloadOnceForScreen(): Promise<boolean> {
  if (alreadyReloaded() || !(await pageAnswers())) return false;
  try {
    sessionStorage.setItem(RELOADED_FOR_SCREEN, "1");
  } catch {
    return false;
  }
  reloadPage();
  return true;
}
