import { useState } from "react";
import Button from "./Button";
import ModalShell, { DialogError } from "./ModalShell";
import PathList from "./PathList";
import PlainButton from "./PlainButton";

/** The directories of the account's Import Runs on this computer, as the dialog shows them. */
export type RunDirectoriesCheck = {
  checking: boolean;
  paths: readonly string[];
  /** Why they could not be looked for, or empty. */
  error: string;
};

/**
 * Confirms an account deleting itself. `hasPassword` is the account's
 * `has_password`: the server checks the current password only when one is
 * set, so the dialog asks for it only then and confirms with none otherwise.
 * `error` is why the last confirm failed; the dialog stays open to retry.
 *
 * `runDirectories`, in the desktop app, are the account's Staging
 * Directories on this computer, deleted with the account. The dialog names them before
 * the person confirms, and holds the confirm while it looks for them.
 *
 * The typed username and password live in `DeleteAccountForm`, which exists
 * only while the dialog is open, so closing the dialog discards them.
 */
export default function DeleteAccountDialog({
  open,
  username,
  hasPassword,
  runDirectories,
  deleting = false,
  error = "",
  onClose,
  onConfirm,
}: {
  open: boolean;
  username: string;
  hasPassword: boolean;
  runDirectories?: RunDirectoriesCheck;
  deleting?: boolean;
  error?: string;
  onClose: () => void;
  onConfirm: (currentPassword?: string) => void;
}) {
  return (
    <ModalShell
      open={open}
      onOpenChange={(o) => {
        if (!o && !deleting) onClose();
      }}
      dismissable={!deleting}
      label="Delete your account?"
    >
      <DeleteAccountForm
        username={username}
        hasPassword={hasPassword}
        runDirectories={runDirectories}
        deleting={deleting}
        error={error}
        onClose={onClose}
        onConfirm={onConfirm}
      />
    </ModalShell>
  );
}

function DeleteAccountForm({
  username,
  hasPassword,
  runDirectories,
  deleting,
  error,
  onClose,
  onConfirm,
}: {
  username: string;
  hasPassword: boolean;
  runDirectories?: RunDirectoriesCheck;
  deleting: boolean;
  error: string;
  onClose: () => void;
  onConfirm: (currentPassword?: string) => void;
}) {
  const [typedUsername, setTypedUsername] = useState("");
  const [password, setPassword] = useState("");

  const expected = username.trim();
  const matches =
    expected.length > 0 && typedUsername === expected && (!hasPassword || password.length > 0);
  const checkingDirectories = runDirectories?.checking ?? false;

  return (
    <>
      <PlainButton
        aria-label="Close"
        isDisabled={deleting}
        onPress={onClose}
        className="absolute top-3 right-3 cursor-pointer border-none bg-transparent text-[1.25rem] leading-none text-muted disabled:cursor-not-allowed"
      >
        ×
      </PlainButton>

      <h2 className="mb-2 pr-6 text-[1rem] font-semibold text-text">Delete your account?</h2>

      <p className="mt-3 text-[0.875rem] leading-relaxed text-muted">
        This cannot be undone. Your messages, contacts, group conversations, profile, and
        attachments will be permanently deleted.
      </p>

      {runDirectories ? <RunDirectoriesNote check={runDirectories} /> : null}

      <label className="mt-5 block">
        <span className="text-[0.875rem] text-text">
          Type your username {expected ? <strong>{expected}</strong> : null} to confirm.
        </span>
        <input
          type="text"
          value={typedUsername}
          onChange={(e) => setTypedUsername(e.target.value)}
          disabled={deleting}
          autoComplete="off"
          spellCheck={false}
          className="mt-2 box-border w-full rounded border border-border bg-elevated px-3 py-2 text-[0.875rem] text-text"
        />
      </label>

      {hasPassword ? (
        <label className="mt-4 block">
          <span className="text-[0.875rem] text-text">Current password</span>
          <input
            type="password"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            disabled={deleting}
            autoComplete="current-password"
            className="mt-2 box-border w-full rounded border border-border bg-elevated px-3 py-2 text-[0.875rem] text-text"
          />
        </label>
      ) : null}

      <DialogError message={error} />

      <div className="mt-5 flex justify-end">
        <Button
          variant="danger"
          disabled={deleting || !matches || checkingDirectories}
          onClick={() => onConfirm(hasPassword ? password : undefined)}
          className="!px-4 !py-2 !text-[0.813rem]"
        >
          {deleting ? "Deleting…" : "Permanently delete my account"}
        </Button>
      </div>
    </>
  );
}

/** Names the Import Runs' directories deleted with the account, or says why it cannot. */
function RunDirectoriesNote({ check }: { check: RunDirectoriesCheck }) {
  const text = "mt-3 text-[0.875rem] leading-relaxed text-muted";
  if (check.checking) {
    return (
      <p className={text}>
        Looking for the directories of this account&apos;s Import Runs on this computer…
      </p>
    );
  }
  if (check.error) {
    return (
      <p className={text}>
        {`Message Crate could not look for the directories of this account's Import Runs on this computer, so it deletes none: ${check.error}`}
      </p>
    );
  }
  if (check.paths.length === 0) return null;
  return (
    <>
      <p className={text}>
        Deleting the account also deletes the directories of its Import Runs on this computer:
      </p>
      <PathList paths={check.paths.map((path) => ({ path }))} />
    </>
  );
}
