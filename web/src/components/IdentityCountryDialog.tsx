import { useEffect, useId, useState } from "react";
import { ApiError } from "../lib/api";
import { usePhoneCountries } from "../lib/phoneCountries";
import { Z_POPOVER_IN_MODAL } from "../lib/zLayers";
import Button from "./Button";
import ModalShell, { DialogError, DialogFooter } from "./ModalShell";
import { phoneCountryItems } from "./phoneCountryItems";
import Select from "./Select";

const fieldLabelClass = "mb-1 block text-[0.813rem] font-medium text-text";
const selectTriggerClass =
  "!box-border !h-9 !min-h-9 !w-full !rounded !px-3 !py-0 !text-[0.875rem] !font-normal !leading-none !bg-elevated";
const selectValueClass = "!text-[0.875rem] !font-normal !leading-none";

/** The problem type the server answers when another identity holds the `+` form. */
const IDENTITY_EXISTS = "identity-exists";

/**
 * The question a refused pick asks, when the refusal is that the `+` form is
 * held. It is the problem's `detail`, which names the number, its `+` form
 * and who holds it in one sentence. The problem's `holder` extension says who
 * holds it for a program; this dialog reads nothing it does not already show,
 * so `holder` is for other clients.
 */
function mergeQuestion(error: Error | null): string | null {
  return error instanceof ApiError && error.type === IDENTITY_EXISTS
    ? (error.detail ?? error.message)
    : null;
}

/**
 * Picking the country of a phone number written without its `+` code (#1676).
 * The contact drawer and My Identities open the same dialog.
 *
 * The number then takes the `+` form that country gives it. When another
 * identity already holds that form, the server refuses and names it, and the
 * dialog asks before it sends the pick again with `merge`, because merging
 * joins the two identities' conversations and cannot be undone.
 */
export default function IdentityCountryDialog({
  open,
  address,
  busy = false,
  error = null,
  onClose,
  onPick,
}: {
  open: boolean;
  /** The number as the list shows it: the digits typed. */
  address: string;
  busy?: boolean;
  /** Why the last pick failed. The dialog stays open so it can be retried. */
  error?: Error | null;
  onClose: () => void;
  onPick: (args: { country: string; merge: boolean }) => void;
}) {
  const { countries, loading, error: listError } = usePhoneCountries();
  const [country, setCountry] = useState<string | null>(null);
  const countryId = useId();

  useEffect(() => {
    if (open) setCountry(null);
  }, [open]);

  const question = mergeQuestion(error);
  const message = question ? "" : (error?.message ?? listError?.message ?? "");
  const canPick = country !== null && !busy;

  return (
    <ModalShell
      open={open}
      onOpenChange={(o) => {
        if (!o && !busy) onClose();
      }}
      dismissable={!busy}
      label="Pick country"
      title="Pick country"
      onClose={onClose}
      closeDisabled={busy}
      maxWidth="26rem"
    >
      <p className="mt-3 text-[0.813rem] leading-snug text-muted">
        {address} was written without its country code, so it matches no number written with one.
        Pick the country it belongs to, and it takes its full form there.
      </p>
      <div className="mt-4">
        <label htmlFor={countryId} className={fieldLabelClass}>
          Country
        </label>
        <Select
          id={countryId}
          selectedKey={country}
          onSelectionChange={(k) => {
            if (typeof k === "string") setCountry(k);
          }}
          aria-label="Country"
          placeholder={loading ? "Loading countries…" : "Pick a country"}
          isDisabled={busy || loading || question !== null}
          triggerClassName={selectTriggerClass}
          valueClassName={selectValueClass}
          popoverClassName={Z_POPOVER_IN_MODAL}
          className="block w-full min-w-0"
        >
          {phoneCountryItems(countries)}
        </Select>
      </div>

      {question ? (
        <div
          role="alert"
          className="mt-3 rounded border border-border bg-elevated px-3 py-2 text-[0.813rem] text-text"
        >
          <p>{question}</p>
          <p className="mt-2 text-muted">
            Merging makes them one identity: their conversations and messages join, and a one-to-one
            conversation with each becomes one.
          </p>
        </div>
      ) : null}
      <DialogError message={message} />

      <DialogFooter>
        <Button onPress={onClose} isDisabled={busy}>
          Cancel
        </Button>
        {question ? (
          <Button
            variant="primary"
            onPress={() => country && onPick({ country, merge: true })}
            isDisabled={!canPick}
          >
            {busy ? "Working…" : "Merge"}
          </Button>
        ) : (
          <Button
            variant="primary"
            onPress={() => country && onPick({ country, merge: false })}
            isDisabled={!canPick}
          >
            {busy ? "Working…" : "Set country"}
          </Button>
        )}
      </DialogFooter>
    </ModalShell>
  );
}
