import { useState } from "react";
import { useNavigate } from "react-router-dom";
import Button from "../../components/Button";
import ConfirmDialog from "../../components/ConfirmDialog";
import DeleteAccountDialog from "../../components/DeleteAccountDialog";
import PlainButton from "../../components/PlainButton";
import { useAuth } from "../../lib/auth";
import { accountRunDirectories } from "../../lib/importRun";
import { keys } from "../../lib/queryKeys";
import { useRouteCache, useRouteQuery } from "../../lib/routeQuery";
import { deleteAccount, deleteAllMessages as deleteAllMessagesRoute } from "../../lib/serverApi";
import { isTauri } from "../../lib/tauri-check";
import { useDeleteAccount, useDeleteAccountMessages } from "../owner/useOwnerAccounts";
import { dangerButtonClass } from "./profileStyles";

const CANNOT_DELETE = "Deleting is not permitted for this account. The owner can delete it.";

const dangerButton = `${dangerButtonClass} !box-border !w-auto !min-w-[10.5rem] !whitespace-nowrap !border-transparent !px-3 !py-2 !text-[0.813rem] !shadow-[inset_0_0_0_1px_var(--danger-soft-border)]`;

/**
 * Delete an account's messages, or the account.
 *
 * For the logged-in account, deleting the account asks for its password when
 * it has one (`hasPassword`) and logs out. In the desktop app it also deletes
 * the directories of the account's Import Runs on this computer, which the dialog names
 * first: the server deletes the account's Import Runs with it, so nothing
 * would offer those directories again. Given `managedAccountId`, the owner is deleting someone
 * else's: no password is asked, because the owner does not know it, and the
 * owner lands back on User Accounts. The owner deletes on the strength of the
 * count and the account holder's word, so the confirmation states the count.
 *
 * `canDelete` is the account's own `delete` permission. Deleting the account
 * deletes its messages, so the server refuses both deletes to an account
 * without it, and the account is told to ask the owner. The owner deletes
 * either way.
 */
export function ProfileDangerZone({
  messagesFixed,
  accountFixed,
  username,
  hasPassword,
  canDelete = true,
  managedAccountId,
  messageCount = 0,
}: {
  /** Nobody deletes the account's messages for good: `fixedSettings(profile).deleteMessages`. */
  messagesFixed: boolean;
  /** The account may not delete itself, and the owner still may: `fixedSettings(profile).deleteOwnAccount`. */
  accountFixed: boolean;
  username: string;
  hasPassword: boolean;
  canDelete?: boolean;
  managedAccountId?: number;
  messageCount?: number;
}) {
  const { logout } = useAuth();
  const navigate = useNavigate();
  const cache = useRouteCache();
  const removeManagedAccount = useDeleteAccount();
  const removeManagedMessages = useDeleteAccountMessages();
  const [dangerZoneOpen, setDangerZoneOpen] = useState(false);
  const [confirmDeleteMessagesOpen, setConfirmDeleteMessagesOpen] = useState(false);
  const [deleteDialogOpen, setDeleteDialogOpen] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const [deletingMessages, setDeletingMessages] = useState(false);
  const [dangerError, setDangerError] = useState("");

  const managed = managedAccountId !== undefined;
  const busy = deleting || deletingMessages;
  const accountLocked = accountFixed && !managed;
  // An account the owner barred from deleting asks the owner instead.
  const notPermitted = !canDelete && !managed;
  const count = messageCount.toLocaleString();
  // Only the desktop app can find and delete directories on this computer.
  const checkDirectories = deleteDialogOpen && !managed && isTauri();
  const runDirectories = useRouteQuery(
    keys.imports.runDirectories,
    (signal) => accountRunDirectories(signal),
    { enabled: checkDirectories, staleTime: 0 },
  );
  // A look that failed deletes nothing, as the dialog says, even with an
  // earlier list still cached.
  const directoriesToDelete =
    checkDirectories && !runDirectories.isError ? (runDirectories.data ?? []) : [];

  const deleteAllMessages = async () => {
    if (messagesFixed || notPermitted) return;
    setDeletingMessages(true);
    setDangerError("");
    try {
      if (managed) await removeManagedMessages.mutateAsync(managedAccountId);
      else await deleteAllMessagesRoute({ confirm: true });
    } catch (e) {
      setDangerError(e instanceof Error ? e.message : String(e));
    } finally {
      // The owner's deletion is a mutation that marks the cache stale itself.
      if (!managed) cache.invalidateAccount();
      setDeletingMessages(false);
      setConfirmDeleteMessagesOpen(false);
    }
  };

  const performDeleteAccount = async (currentPassword?: string) => {
    if (accountLocked || notPermitted) return;
    setDeleting(true);
    setDangerError("");
    try {
      if (managed) {
        await removeManagedAccount.mutateAsync(managedAccountId);
        setDeleteDialogOpen(false);
        navigate("/owner/accounts");
        return;
      }
      await deleteAccount({ confirm: true, current_password: currentPassword });
      setDeleteDialogOpen(false);
      // The account is gone, so there is nothing to go back to: an Upload
      // still running is paused without asking, and the directories the dialog
      // named are deleted once the session is revoked.
      void logout({
        ask: false,
        deletedAccountDirectories: directoriesToDelete,
      });
    } catch (e) {
      setDangerError(e instanceof Error ? e.message : String(e));
    } finally {
      setDeleting(false);
    }
  };

  return (
    <>
      <section className="mt-8 border-t border-border pt-6">
        <PlainButton
          aria-expanded={dangerZoneOpen}
          onPress={() => setDangerZoneOpen((open) => !open)}
          className="flex w-full cursor-pointer items-center gap-2 border-none bg-transparent p-0 text-left"
        >
          <span
            className={`inline-block text-[0.75rem] text-danger transition-transform duration-150 ${
              dangerZoneOpen ? "rotate-90" : ""
            }`}
          >
            ▶
          </span>
          <span className="text-[0.75rem] font-semibold uppercase tracking-[0.06em] text-danger">
            Danger zone
          </span>
        </PlainButton>
        <p className="ml-5 mt-[0.35rem] text-[0.813rem] text-muted">
          {managed
            ? `Delete ${username}'s messages or permanently remove the account.`
            : "Delete messages or permanently remove your account."}
        </p>
        {notPermitted && (
          <p className="ml-5 mt-[0.35rem] text-[0.813rem] text-muted">
            Ask the owner to delete your messages or your account.
          </p>
        )}

        {dangerZoneOpen && (
          <div className="ml-5 mt-4 rounded-xl border-solid border-danger p-5 [border-width:0.75px]">
            <div className="grid grid-cols-[minmax(0,1fr)_auto] items-center gap-x-5 gap-y-5">
              <div className="min-w-0">
                <p className="m-0 text-[0.813rem] font-bold text-text">
                  Delete all messages &amp; attachments
                </p>
                <p className="m-0 mt-0.5 text-[0.813rem] text-muted">
                  {managed
                    ? "The account, its contacts and its settings remain"
                    : "Your contacts and settings remain"}
                </p>
              </div>
              <div className="justify-self-end p-px">
                <Button
                  variant="danger"
                  disabled={busy || messagesFixed || notPermitted}
                  onClick={() => setConfirmDeleteMessagesOpen(true)}
                  className={dangerButton}
                  title={
                    messagesFixed
                      ? "Unavailable on the Demo Account"
                      : notPermitted
                        ? CANNOT_DELETE
                        : undefined
                  }
                >
                  {deletingMessages ? "Deleting…" : "Delete all messages"}
                </Button>
              </div>

              <div className="min-w-0">
                <p className="m-0 text-[0.813rem] font-bold text-text">
                  {managed ? "Delete this account" : "Delete your account"}
                </p>
                <p className="m-0 mt-0.5 text-[0.813rem] text-muted">
                  Permanently delete all contacts, messages, and attachments. This can&apos;t be
                  undone.
                </p>
              </div>
              <div className="justify-self-end p-px">
                <Button
                  variant="danger"
                  disabled={busy || accountLocked || notPermitted}
                  onClick={() => {
                    setDangerError("");
                    setDeleteDialogOpen(true);
                  }}
                  className={dangerButton}
                  title={
                    accountLocked
                      ? "Unavailable on the Demo Account"
                      : notPermitted
                        ? CANNOT_DELETE
                        : undefined
                  }
                >
                  Delete account
                </Button>
              </div>

              {dangerError && (
                <div className="col-span-2 text-[0.813rem] text-danger" role="alert">
                  {dangerError}
                </div>
              )}
            </div>
          </div>
        )}
      </section>

      {managed ? (
        <ConfirmDialog
          open={deleteDialogOpen}
          title={`Delete ${username}'s account?`}
          body={`This permanently deletes ${username}'s account along with ${count} messages and their attachments. It cannot be undone.`}
          confirmLabel="Delete account"
          danger
          busy={deleting}
          error={dangerError}
          onClose={() => {
            if (!deleting) setDeleteDialogOpen(false);
          }}
          onConfirm={() => void performDeleteAccount()}
        />
      ) : (
        <DeleteAccountDialog
          open={deleteDialogOpen}
          username={username}
          hasPassword={hasPassword}
          runDirectories={
            checkDirectories
              ? {
                  // A look still under way holds the confirm, even with an earlier
                  // list cached: that list may miss a directory made since.
                  checking: runDirectories.isFetching,
                  paths: directoriesToDelete,
                  error: runDirectories.error?.message ?? "",
                }
              : undefined
          }
          deleting={deleting}
          error={dangerError}
          onClose={() => {
            if (!deleting) setDeleteDialogOpen(false);
          }}
          onConfirm={(password) => void performDeleteAccount(password)}
        />
      )}

      <ConfirmDialog
        open={confirmDeleteMessagesOpen}
        title={managed ? `Delete ${username}'s messages?` : "Delete all messages?"}
        body={
          managed
            ? `This permanently deletes ${count} messages belonging to ${username}, and their attachments. It cannot be undone.`
            : "Delete all messages and attachments? Your contacts and settings will remain."
        }
        confirmLabel="Delete all messages"
        danger
        busy={deletingMessages}
        onClose={() => setConfirmDeleteMessagesOpen(false)}
        onConfirm={() => void deleteAllMessages()}
      />
    </>
  );
}
