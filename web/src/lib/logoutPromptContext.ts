/**
 * What logging out has to show the person, as `LogoutDialog` reads it.
 *
 * `AuthProvider` runs the logout and fills this context; the dialog that
 * shows it lives in `components/` and `components/AuthShell.tsx` places it
 * beside the provider, so `lib/` renders nothing of its own (#2214).
 */
import { createContext, useContext } from "react";
import type { DirectoryReason } from "./importRun";

/**
 * Where logging out is: the question it asks while an Upload runs, the wait
 * while that Upload pauses, or what it says once the person is logged out.
 */
export type LogoutPromptState =
  | { kind: "asking" }
  /** `accountDeleted`: the account was just deleted, so nothing resumes. */
  | { kind: "pausing"; accountDeleted: boolean }
  /** Logged out before the Upload had paused; it resumes from what it sent. */
  | { kind: "notPaused" }
  /**
   * Directories of a deleted account's Import Runs left in place: `undeleted`,
   * the ones that could not be deleted, and `unreadable`, the ones Message
   * Crate may not read, which may not be on this computer at all.
   */
  | {
      kind: "leftDirectories";
      undeleted: readonly DirectoryReason[];
      unreadable: readonly DirectoryReason[];
    };

interface LogoutPrompt {
  /** What to show, or null when logging out has nothing to show. */
  state: LogoutPromptState | null;
  /** The person chose to log out while an Upload runs. */
  logOut: () => void;
  /** The person chose to leave the Upload running and stay logged in. */
  goBack: () => void;
  /** The person chose not to wait for the Upload to pause. */
  logOutNow: () => void;
  /** The person has read what logging out said. */
  dismiss: () => void;
}

export const LogoutPromptContext = createContext<LogoutPrompt | null>(null);

/** What logging out shows. Must be called under AuthProvider. */
export function useLogoutPrompt(): LogoutPrompt {
  const ctx = useContext(LogoutPromptContext);
  if (!ctx) throw new Error("useLogoutPrompt must be used within AuthProvider");
  return ctx;
}
