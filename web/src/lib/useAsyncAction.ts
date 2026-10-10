import { useCallback, useState } from "react";
import { errorText } from "./apiErrorMessage";

/**
 * Run one async action at a time and keep a busy flag plus an error string.
 * A new `run` call sets busy, clears the previous error, then stores any failure.
 */
export function useAsyncAction(): {
  busy: boolean;
  error: string;
  run: (fn: () => Promise<void>) => Promise<void>;
  clearError: () => void;
} {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  const clearError = useCallback(() => setError(""), []);

  const run = useCallback(async (fn: () => Promise<void>) => {
    setBusy(true);
    setError("");
    try {
      await fn();
    } catch (e: unknown) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  }, []);

  return { busy, error, run, clearError };
}
