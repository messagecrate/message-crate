import { useMemo, useState } from "react";
import Button from "../../components/Button";
import ConfirmDialog from "../../components/ConfirmDialog";
import IdentityDialogs from "../../components/IdentityDialogs";
import IdentityTable, { type IdentityRow } from "../../components/IdentityTable";
import { useIdentityCountryPick } from "../../hooks/useIdentityCountryPick";
import { useRouteCache, useRouteQuery } from "../../hooks/useRouteQuery";
import { type AccountProfile, fixedSettings } from "../../lib/account";
import { errorLine } from "../../lib/apiErrorMessage";
import { listedServerService, type OfferedService, serverService } from "../../lib/offeredService";
import { keys } from "../../lib/queryKeys";
import { listAccountIdentities } from "../../lib/serverApi";
import { type Identity, removeBody } from "./identities";
import { sectionTitleClass } from "./profileStyles";
import { useUpdateSettingsProfile } from "./useSettingsAccount";

/** Whether `list` has a row at `address` on `service`, both as the server lists them. */
function listHolds(list: readonly Identity[], { address, service }: ListedIdentity): boolean {
  return list.some((row) => row.address === address && row.service === service);
}

/**
 * Whether `stored`, an address as the server lists it, is `typed` as the
 * person typed it. The server stores an address in lower case, and a number
 * with its country code when the country is known, so a number matches when
 * the stored digits end with the typed ones, less any leading trunk `0`
 * (`07700 900123` is `+447700900123`, `(555) 555-0100` is `+15555550100`).
 */
function matchesStored(typed: string, stored: string): boolean {
  const trimmed = typed.trim();
  if (stored === trimmed.toLowerCase()) return true;
  if (trimmed.includes("@")) return false;
  const digits = trimmed.replace(/\D/g, "").replace(/^0+/, "");
  return digits.length > 0 && stored.replace(/\D/g, "").endsWith(digits);
}

/** An identity as the identities list names it: its stored address and listed service. */
type ListedIdentity = Pick<Identity, "address" | "service">;

/** The profile's own identities as placeholder rows, shown until the server lists them. */
function placeholderRows(profile: AccountProfile): Identity[] {
  return [
    ...profile.phones.flatMap(({ address, services }) =>
      services.map((service) => ({ address, service })),
    ),
    ...profile.emails.map((address) => ({ address, service: "email" })),
  ].map((row) => ({
    ...row,
    country_unknown: false,
    start_date: null,
    end_date: null,
    conversations: 0,
    one_to_one_messages: 0,
    group_messages: 0,
    orphaned_messages: 0,
  }));
}

/**
 * The account's own identities: the addresses whose messages are the account
 * holder's. Import uses them to decide which messages belong to the holder.
 *
 * Given `managedAccountId`, they are an account's the administrator opened from
 * User Accounts, and the administrator adds and removes them as the holder does.
 */
export function IdentitiesSection({
  profile,
  managedAccountId,
}: {
  profile: AccountProfile;
  managedAccountId?: number;
}) {
  const managed = managedAccountId !== undefined;
  const fixed = fixedSettings(profile).identities;
  const updateProfile = useUpdateSettingsProfile(managedAccountId);
  const [adding, setAdding] = useState(false);
  const [addError, setAddError] = useState("");
  const [removeTarget, setRemoveTarget] = useState<Identity | null>(null);
  const [removeError, setRemoveError] = useState("");
  const busy = updateProfile.isPending;
  const countryPick = useIdentityCountryPick((pick) =>
    updateProfile.mutateAsync({ set_identity_country: pick }),
  );

  // Until the server answers, the profile's own identities are shown with no
  // counts, so the table never waits on a fetch and never shows a number
  // that is not the server's.
  const identitiesKey = managed
    ? keys.adminAccounts.identities(managedAccountId)
    : keys.accountProfile.identities;
  const fetchIdentities = (signal: AbortSignal) =>
    listAccountIdentities({ signal }, managedAccountId);
  const identities = useRouteQuery(identitiesKey, fetchIdentities);
  const cache = useRouteCache();
  const refetchIdentities = () => cache.fetch(identitiesKey, fetchIdentities);
  const listed = identities.data;
  const rows: Identity[] = useMemo(() => listed ?? placeholderRows(profile), [listed, profile]);
  const tableRows: IdentityRow[] = useMemo(
    () =>
      rows.map((row) => ({
        ...row,
        start_date: row.start_date ?? null,
        end_date: row.end_date ?? null,
      })),
    [rows],
  );

  /**
   * Send `body`, then read the identities list the table shows again and
   * require `changed` of it. A server that answers without making the change
   * leaves the dialog open with `notChanged`.
   */
  const changeAndCheck = async (
    body: Parameters<typeof updateProfile.mutateAsync>[0],
    changed: (list: Identity[]) => boolean,
    notChanged: string,
  ) => {
    await updateProfile.mutateAsync(body);
    let list: Identity[];
    try {
      list = await refetchIdentities();
    } catch (e) {
      throw new Error(
        `The change was sent, but the identities list could not be read to confirm it. ${errorLine(e)}`,
        { cause: e },
      );
    }
    if (!changed(list)) {
      throw new Error(notChanged);
    }
  };

  const confirmAdd = async ({ address, service }: { address: string; service: OfferedService }) => {
    setAddError("");
    try {
      // The list names each service as the dialog offers it. The address is
      // the server's own form: a row on `service` matches when it is the
      // typed address in that form, or is new, since a number typed without
      // its country code is stored with the code the account's country gives.
      const before = listed ?? (await refetchIdentities());
      await changeAndCheck(
        { identities: [{ address, service: serverService(service) }] },
        (list) =>
          list.some(
            (row) =>
              row.service === service &&
              (matchesStored(address, row.address) || !listHolds(before, row)),
          ),
        "The server did not add that identity.",
      );
      setAdding(false);
    } catch (e) {
      setAddError(errorLine(e));
    }
  };

  const confirmRemove = async () => {
    if (!removeTarget) return;
    const { address } = removeTarget;
    setRemoveError("");
    try {
      const service = listedServerService(removeTarget.service);
      if (service === undefined) {
        throw new Error(
          `The server listed ${address} on a service it does not take: ${removeTarget.service}.`,
        );
      }
      await changeAndCheck(
        { remove_identities: [{ address, service }] },
        (list) => !listHolds(list, removeTarget),
        "The server did not remove that identity.",
      );
      setRemoveTarget(null);
    } catch (e) {
      setRemoveError(errorLine(e));
    }
  };

  const requestRemove = (row: IdentityRow) => {
    const target = rows.find((r) => r.address === row.address && r.service === row.service);
    if (target) {
      setRemoveError("");
      setRemoveTarget(target);
    }
  };

  return (
    <>
      <h3 className={sectionTitleClass}>{managed ? "Identities" : "My Identities"}</h3>
      <p className="mt-0 mb-3 text-[0.813rem] text-muted">
        {managed
          ? "The account holder's phone numbers and emails. Import uses them to determine which messages belong to them."
          : "Your phone numbers and emails. Import uses them to determine which messages belong to you."}
      </p>
      <div className="overflow-x-auto">
        <IdentityTable
          ariaLabel="Identities"
          // A person does not hear from themselves: these are the dates their
          // own messages were sent from the identity.
          firstDateHeading="First sent"
          lastDateHeading="Last sent"
          rows={tableRows}
          loading={listed === undefined}
          busy={busy}
          emptyText="No identities yet"
          onRemove={fixed ? undefined : requestRemove}
          onPickCountry={
            fixed
              ? undefined
              : (row) => countryPick.request({ address: row.address, service: row.service })
          }
        />
      </div>
      <div className="mt-3 mb-6">
        {fixed ? (
          <p className="m-0 text-[0.813rem] text-muted">The Demo Account's identities are fixed.</p>
        ) : (
          <Button
            variant="primary"
            size="sm"
            isDisabled={busy}
            onPress={() => {
              setAddError("");
              setAdding(true);
            }}
          >
            Add identity
          </Button>
        )}
      </div>

      <IdentityDialogs
        countryPick={countryPick}
        busy={busy}
        adding={adding}
        addError={addError}
        existing={rows}
        onCloseAdd={() => setAdding(false)}
        onConfirmAdd={confirmAdd}
      />
      <ConfirmDialog
        open={removeTarget !== null}
        title="Remove identity?"
        body={removeTarget ? removeBody(removeTarget) : null}
        confirmLabel="Remove"
        danger
        busy={busy}
        error={removeTarget ? removeError : ""}
        onClose={() => {
          if (!busy) setRemoveTarget(null);
        }}
        onConfirm={() => void confirmRemove()}
      />
    </>
  );
}
