import type { ReactNode } from "react";
import type { DirectoryReason } from "../lib/importRun";
import Button from "./Button";
import ConfirmDialog from "./ConfirmDialog";
import ModalShell, { DialogFooter } from "./ModalShell";
import PathList from "./PathList";
import { useLogoutPrompt } from "./useLogoutPrompt";

const UPLOAD_RUNNING_PROMPT =
  "An Upload is running. Logging out pauses it; you can resume it after you log in.";

const UPLOAD_PAUSING =
  "Pausing the Upload. You are logged out once it has paused, or after 15 seconds.";

const DELETED_ACCOUNT_UPLOAD_PAUSING =
  "Your account is deleted. Stopping its Upload before you are logged out, which takes at most 15 seconds.";

const UNDELETED_DIRECTORIES =
  "Message Crate could not delete these directories of the deleted account's Import Runs. Delete them by hand to free the space they take:";

const UNREADABLE_DIRECTORIES =
  "Message Crate could not tell whether these directories of the deleted account's Import Runs are on this computer. If they are, delete them by hand:";

const UPLOAD_NOT_PAUSED =
  "You were logged out before the Upload had paused. When you log in again, it resumes from what it had sent.";

/**
 * The dialog logout shows, read from `useLogoutPrompt`, or nothing while
 * logging out has nothing to show. `AuthShell` places it once, beside what
 * `AuthProvider` wraps.
 *
 * While the Upload pauses there is nothing to go back to, so the only
 * button is **Log out now**, which stops waiting.
 */
export default function LogoutDialog() {
  const { state, logOut, goBack, logOutNow, dismiss } = useLogoutPrompt();
  if (state?.kind === "pausing") {
    return (
      <ModalShell
        open
        onOpenChange={() => {}}
        dismissable={false}
        keyboardDismissable={false}
        label="Log out"
        title="Log out"
        maxWidth="24rem"
      >
        <p className="mt-3 text-[0.875rem] leading-relaxed text-muted">
          {state.accountDeleted ? DELETED_ACCOUNT_UPLOAD_PAUSING : UPLOAD_PAUSING}
        </p>
        <DialogFooter>
          <Button variant="primary" onPress={logOutNow}>
            Log out now
          </Button>
        </DialogFooter>
      </ModalShell>
    );
  }
  if (state?.kind === "notPaused") {
    return (
      <Notice title="Logged out" onDismiss={dismiss}>
        <p className="mt-3 text-[0.875rem] leading-relaxed text-muted">{UPLOAD_NOT_PAUSED}</p>
      </Notice>
    );
  }
  if (state?.kind === "leftDirectories") {
    return (
      <Notice
        title={
          state.undeleted.length > 0
            ? "Directories of Import Runs left on this computer"
            : "Directories of Import Runs that may be left on this computer"
        }
        onDismiss={dismiss}
      >
        <LeftDirectories lead={UNDELETED_DIRECTORIES} directories={state.undeleted} />
        {/* These may hold the account's data, or the run may have been on
            another computer, so the notice does not say they are here. */}
        <LeftDirectories lead={UNREADABLE_DIRECTORIES} directories={state.unreadable} />
      </Notice>
    );
  }
  return (
    <ConfirmDialog
      open={state?.kind === "asking"}
      title="Log out"
      body={UPLOAD_RUNNING_PROMPT}
      confirmLabel="Log out"
      cancelLabel="Go back"
      onConfirm={logOut}
      onClose={goBack}
    />
  );
}

/** What logout says once the person is logged out, with an **OK** to close it. */
function Notice({
  title,
  onDismiss,
  children,
}: {
  title: string;
  onDismiss: () => void;
  children: ReactNode;
}) {
  return (
    <ModalShell
      open
      onOpenChange={(open) => {
        if (!open) onDismiss();
      }}
      label={title}
      title={title}
      onClose={onDismiss}
      maxWidth="28rem"
    >
      {children}
      <DialogFooter>
        <Button variant="primary" onPress={onDismiss}>
          OK
        </Button>
      </DialogFooter>
    </ModalShell>
  );
}

/** Names each Import Run's directory of a deleted account in `directories`, and why, after `lead`. */
function LeftDirectories({
  lead,
  directories,
}: {
  lead: string;
  directories: readonly DirectoryReason[];
}) {
  if (directories.length === 0) return null;
  return (
    <>
      <p className="mt-3 text-[0.875rem] leading-relaxed text-muted">{lead}</p>
      <PathList paths={directories.map(({ path, reason }) => ({ path, note: reason }))} />
    </>
  );
}
