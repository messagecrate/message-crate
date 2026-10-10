import type { OfferedService } from "../lib/offeredService";
import AddIdentityDialog from "./AddIdentityDialog";
import IdentityCountryDialog from "./IdentityCountryDialog";
import type { useIdentityCountryPick } from "./useIdentityCountryPick";

/**
 * The Pick country dialog and the Add identity dialog of one identity table.
 * The contact drawer and My Identities render them here, so the two dialogs
 * open and close the same way in both.
 *
 * The Pick country dialog's state comes whole from `useIdentityCountryPick`,
 * which both editors use. The Add identity dialog's state comes as separate
 * props because each editor keeps it its own way: the drawer in
 * `useHandleMutations`, with one error for every change, and My Identities in
 * its own state, with an error for adding alone.
 *
 * Neither dialog closes while `busy`: a change the server has not answered
 * yet keeps its dialog open, so its answer is shown where it was asked.
 */
export default function IdentityDialogs({
  countryPick,
  busy,
  adding,
  addError,
  existing,
  onCloseAdd,
  onConfirmAdd,
}: {
  /** The Pick country dialog's state, from `useIdentityCountryPick`. */
  countryPick: Pick<
    ReturnType<typeof useIdentityCountryPick>,
    "target" | "error" | "close" | "confirm"
  >;
  busy: boolean;
  /** Whether the Add identity dialog is open. */
  adding: boolean;
  /** Why the last add failed. */
  addError: string;
  /** The identities already in the table, which the Add identity dialog refuses again. */
  existing: readonly { address: string; service?: string | null }[];
  onCloseAdd: () => void;
  /**
   * Add the identity. A promise it returns is not awaited, so the editor
   * reports its own failure in `addError`. The dialog stays open until the
   * editor closes it.
   */
  onConfirmAdd: (args: { address: string; service: OfferedService }) => void | Promise<void>;
}) {
  return (
    <>
      <IdentityCountryDialog
        open={countryPick.target !== null}
        address={countryPick.target?.address ?? ""}
        busy={busy}
        error={countryPick.error}
        onClose={() => {
          if (!busy) countryPick.close();
        }}
        onPick={(args) => void countryPick.confirm(args)}
      />
      <AddIdentityDialog
        open={adding}
        busy={busy}
        error={addError}
        existing={existing}
        onClose={() => {
          if (!busy) onCloseAdd();
        }}
        onConfirm={(args) => void onConfirmAdd(args)}
      />
    </>
  );
}
