import { type ComponentType, type LazyExoticComponent, lazy } from "react";
import { errorText } from "./apiErrorMessage";
import { pageAnswers, reloadPage } from "./pageReload";

/**
 * The session storage entry that says this tab already reloaded once for the
 * chunk `name` that didn't load, so a chunk that fails again shows why instead
 * of reloading forever. One entry per chunk, so another chunk that loads
 * never clears it.
 */
function reloadedKey(name: string): string {
  return `mc-reloaded-for-chunk:${name}`;
}

/**
 * A screen's or a panel's code that didn't arrive. Its usual cause is a new
 * release: the server replaced the app's files, and a tab opened before the
 * upgrade asks for a file by a name the server no longer has. The browser's
 * own error (`cause`) is the message.
 */
export class ChunkLoadError extends Error {
  /** The name `lazyChunk` was given, such as `SettingsScreen`. */
  readonly chunk: string;

  constructor(chunk: string, cause: unknown) {
    super(errorText(cause), { cause });
    this.name = "ChunkLoadError";
    this.chunk = chunk;
  }
}

function forgetReload(name: string): void {
  try {
    sessionStorage.removeItem(reloadedKey(name));
  } catch {
    // Storage the browser refuses holds no mark to forget.
  }
}

/**
 * `lazy`, for a screen or a panel, such as Settings or Advanced search, whose
 * code is downloaded when it is first shown. `name` tells it from the others.
 * A download that fails rejects with `ChunkLoadError`, which `ErrorBoundary`
 * answers by reloading the page once (`reloadOnceForChunk`). A download of
 * the same chunk that arrives clears the mark that reload left, so a later
 * release reloads again.
 */
export function lazyChunk<P>(
  name: string,
  load: () => Promise<{ default: ComponentType<P> }>,
): LazyExoticComponent<ComponentType<P>> {
  return lazy(() =>
    load().then(
      (chunk) => {
        forgetReload(name);
        return chunk;
      },
      (e: unknown) => {
        throw new ChunkLoadError(name, e);
      },
    ),
  );
}

function alreadyReloaded(name: string): boolean {
  try {
    return sessionStorage.getItem(reloadedKey(name)) !== null;
  } catch {
    // Nothing would stop the next failure from reloading again.
    return true;
  }
}

/**
 * Reload the page for a chunk that didn't load, unless this tab already did
 * for that chunk since it last loaded, or the page itself no longer answers.
 * A reload fetches the current release's files, which is what code from an
 * earlier release needs. A page that doesn't answer, such as the website
 * while its server is down, would reload into the browser's own error page,
 * so the person keeps the app and its message instead. A browser that refuses
 * session storage gets no reload either, because nothing would stop the next
 * failure from reloading again. Resolves to whether it reloaded.
 */
export async function reloadOnceForChunk(error: ChunkLoadError): Promise<boolean> {
  if (alreadyReloaded(error.chunk) || !(await pageAnswers())) return false;
  try {
    sessionStorage.setItem(reloadedKey(error.chunk), "1");
  } catch {
    return false;
  }
  reloadPage();
  return true;
}
