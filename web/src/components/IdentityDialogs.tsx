import type { OfferedService } from "../lib/offeredService";
import AddIdentityDialog from "./AddIdentityDialog";
import IdentityCountryDialog from "./IdentityCountryDialog";
import type { useIdentityCountryPick } from "./useIdentityCountryPick";

/**
 * The Pick country dialog and the Add identity dialog of one identity table.
 * The contact drawer and Settings, Identities render them here, so the two
 * dialogs open and close the same way in both.
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
  onConfirmAdd: (args: { address: string; service: OfferedService }) => void;
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
        onConfirm={onConfirmAdd}
      />
    </>
  );
}
