import { useQuery } from "@tanstack/react-query";
import { useServerInfo } from "../../hooks/useServerInfo";
import { errorLine } from "../../lib/apiErrorMessage";
import { useAuth } from "../../lib/authContext";
import { invokeAccountStagingBytes, invokeAllStagingBytes } from "../../lib/desktop/staging";
import { canMeasureStagingDirectory } from "../../lib/desktopFeatures";
import { formatBytes } from "../../lib/formatBytes";
import { keys } from "../../lib/queryKeys";
import { isTauri } from "../../lib/tauriCheck";

/**
 * How a measurement of the Staging Directory reads: measured once when the
 * screen opens, never again while it is open (#3087). A run that ends while
 * the screen is open changes nothing on it until it is opened again.
 */
const MEASURED_ONCE = {
  staleTime: Number.POSITIVE_INFINITY,
  refetchOnMount: "always",
  refetchOnWindowFocus: false,
  retry: false,
} as const;

/** One line of the measurement: the bytes, or why there are none yet. */
function UsageLine({
  bytes,
  error,
  says,
  className,
}: {
  bytes: number | undefined;
  error: unknown;
  says: (size: string) => string;
  className: string;
}) {
  if (error) {
    return (
      <p className={`${className} m-0 text-danger`} role="alert">
        Could not measure the Staging Directory. {errorLine(error)}
      </p>
    );
  }
  return (
    <p className={`${className} m-0 text-muted`}>
      {bytes === undefined ? "Measuring…" : says(formatBytes(bytes))}
    </p>
  );
}

/**
 * The disk space the signed-in account's Import Runs take in the Staging
 * Directory on this computer. Desktop app only: the directory is on the
 * computer running the app, and a browser cannot see it.
 *
 * Which runs are the account's depends on the Message Crate it is signed in
 * to, so a server that does not say which it is leaves nothing to measure,
 * and the line says why.
 */
export function AccountStagingUsage({ className }: { className: string }) {
  const { accountId } = useAuth();
  const server = useServerInfo();
  const messageCrateId = server.data?.id;
  const desktop = canMeasureStagingDirectory(isTauri());
  const known = accountId !== null && messageCrateId !== undefined;
  const usage = useQuery({
    queryKey: known
      ? keys.desktopStagingBytes.of(accountId, messageCrateId)
      : keys.desktopStagingBytes.unknown,
    queryFn: () => (known ? invokeAccountStagingBytes(accountId, messageCrateId) : 0),
    enabled: desktop && known,
    ...MEASURED_ONCE,
  });
  if (!desktop) return null;
  return (
    <UsageLine
      bytes={usage.data}
      // The server's error only when it is why nothing was measured: a later
      // read of the server that fails leaves a measurement already made.
      error={usage.error ?? (messageCrateId === undefined ? server.error : null)}
      says={(size) => `Your Import Runs on this computer take up ${size}.`}
      className={className}
    />
  );
}

/**
 * The disk space the whole Staging Directory takes on this computer, for
 * the owner on Server Settings: every account's Import Runs, and anything
 * else in it. Desktop app only.
 */
export function AllStagingUsage() {
  const desktop = canMeasureStagingDirectory(isTauri());
  const usage = useQuery({
    queryKey: keys.desktopStagingBytes.all,
    queryFn: invokeAllStagingBytes,
    enabled: desktop,
    ...MEASURED_ONCE,
  });
  if (!desktop) return null;
  return (
    <section className="mt-4 rounded-xl border border-border bg-elevated p-4">
      <h4 className="m-0 text-[0.875rem] font-semibold text-text">
        Staging Directory on this computer
      </h4>
      <UsageLine
        bytes={usage.data}
        error={usage.error}
        says={(size) => `The Staging Directory takes up ${size} on this computer.`}
        className="mt-2 text-[0.813rem]"
      />
      <p className="mt-2 mb-0 text-[0.75rem] text-muted">
        It holds the Import Runs of every account that imports from this computer, into this Message
        Crate or another. Measured when this page opens. Each account sees what its own Import Runs
        take in Settings → System.
      </p>
    </section>
  );
}
