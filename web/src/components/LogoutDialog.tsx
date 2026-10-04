import type { ReactNode } from "react";
import Button from "./Button";
import ConfirmDialog from "./ConfirmDialog";
import ModalShell, { DialogFooter } from "./ModalShell";

/**
 * What logging out shows: the question it asks while an Upload runs, the
 * wait while that Upload pauses, or a notice once the person is logged out.
 */
export type LogoutDialogState =
  | { kind: "asking" }
  /** `accountDeleted`: the account was just deleted, so nothing resumes. */
  | { kind: "pausing"; accountDeleted: boolean }
  | { kind: "notice"; title: string; body: ReactNode };

const UPLOAD_RUNNING_PROMPT =
  "An Upload is running. Logging out pauses it; you can resume it after you log in.";

const UPLOAD_PAUSING =
  "Pausing the Upload. You are logged out once it has paused, or after 15 seconds.";

const DELETED_ACCOUNT_UPLOAD_PAUSING =
  "Your account is deleted. Stopping its Upload before you are logged out, which takes at most 15 seconds.";

/**
 * The dialog logout shows, or nothing when `state` is null.
 *
 * While the Upload pauses there is nothing to go back to, so the only
 * button is **Log out now**, which stops waiting.
 */
export default function LogoutDialog({
  state,
  onLogOut,
  onGoBack,
  onLogOutNow,
  onDismiss,
}: {
  state: LogoutDialogState | null;
  /** The person chose to log out while an Upload runs. */
  onLogOut: () => void;
  /** The person chose to leave the Upload running and stay logged in. */
  onGoBack: () => void;
  /** The person chose not to wait for the Upload to pause. */
  onLogOutNow: () => void;
  /** The person has read the notice. */
  onDismiss: () => void;
}) {
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
          <Button variant="primary" onPress={onLogOutNow}>
            Log out now
          </Button>
        </DialogFooter>
      </ModalShell>
    );
  }
  if (state?.kind === "notice") {
    return (
      <ModalShell
        open
        onOpenChange={(open) => {
          if (!open) onDismiss();
        }}
        label={state.title}
        title={state.title}
        onClose={onDismiss}
        maxWidth="28rem"
      >
        {state.body}
        <DialogFooter>
          <Button variant="primary" onPress={onDismiss}>
            OK
          </Button>
        </DialogFooter>
      </ModalShell>
    );
  }
  return (
    <ConfirmDialog
      open={state?.kind === "asking"}
      title="Log out"
      body={UPLOAD_RUNNING_PROMPT}
      confirmLabel="Log out"
      cancelLabel="Go back"
      onConfirm={onLogOut}
      onClose={onGoBack}
    />
  );
}
