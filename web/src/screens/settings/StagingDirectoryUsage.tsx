import { useQuery } from "@tanstack/react-query";
import { errorLine } from "../../lib/apiErrorMessage";
import { useAuth } from "../../lib/authContext";
import { invokeAccountStagingBytes, invokeAllStagingBytes } from "../../lib/desktop/staging";
import { canMeasureStagingDirectory } from "../../lib/desktopFeatures";
import { formatBytes } from "../../lib/formatBytes";
import { keys } from "../../lib/queryKeys";
import { isTauri } from "../../lib/tauriCheck";
import { useServerInfo } from "../../lib/useServerInfo";

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
 */
export function AccountStagingUsage({ className }: { className: string }) {
  const { accountId } = useAuth();
  const messageCrateId = useServerInfo().data?.id;
  const desktop = canMeasureStagingDirectory(isTauri());
  const usage = useQuery({
    queryKey: keys.desktopStagingBytes.of(accountId ?? 0, messageCrateId ?? ""),
    queryFn: () => invokeAccountStagingBytes(accountId ?? 0, messageCrateId ?? ""),
    enabled: desktop && accountId !== null && messageCrateId !== undefined,
    ...MEASURED_ONCE,
  });
  if (!desktop) return null;
  return (
    <UsageLine
      bytes={usage.data}
      error={usage.error}
      says={(size) => `Your Import Runs on this computer take up ${size}.`}
      className={className}
    />
  );
}

/**
 * The disk space every Import Run on this computer takes in the Staging
 * Directory, for every account, for the owner. Desktop app only.
 */
export function AllStagingUsage({ className }: { className: string }) {
  const desktop = canMeasureStagingDirectory(isTauri());
  const usage = useQuery({
    queryKey: keys.desktopStagingBytes.all,
    queryFn: invokeAllStagingBytes,
    enabled: desktop,
    ...MEASURED_ONCE,
  });
  if (!desktop) return null;
  return (
    <UsageLine
      bytes={usage.data}
      error={usage.error}
      says={(size) => `Import Runs of every account take up ${size} on this computer.`}
      className={className}
    />
  );
}
