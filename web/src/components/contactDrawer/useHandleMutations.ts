import { useState } from "react";
import { type ContactHandle, useUpdateContact } from "../../lib/contactDetail";
import { listedServerService, type OfferedService, serverService } from "../../lib/offeredService";
import { formatOfferedServiceLabel } from "./contactDrawerTypes";
import type { RemoveIdentityTarget } from "./handleTableLogic";

/**
 * The add and remove dialogs of a contact's handle table, and the update they send.
 * `ContactDrawer` keys the table by contact id, so this state starts fresh for each contact.
 */
export function useHandleMutations({ contactId }: { contactId: string }) {
  const [adding, setAdding] = useState(false);
  const [removeTarget, setRemoveTarget] = useState<RemoveIdentityTarget | null>(null);
  // The identity whose country is being picked, as the list shows it.
  const [countryTarget, setCountryTarget] = useState<{
    address: string;
    service: string | null;
  } | null>(null);
  const updateContact = useUpdateContact();
  const busy = updateContact.isPending;
  // The dialogs stay open on a refusal and show this, so a person can retry.
  const error = updateContact.error ? updateContact.error.message : "";

  const requestRemoveHandle = (h: ContactHandle) => {
    if (busy) return;
    setRemoveTarget({
      address: h.address,
      service: h.service ?? null,
      serviceLabel: formatOfferedServiceLabel(h.address, h.service),
      conversationCount: h.conversations,
    });
  };

  const confirmRemoveHandle = () => {
    if (!removeTarget || busy) return;
    const address = removeTarget.address;
    // An email address names no service: the list calls it `email` whatever
    // service it is on, and an import can store one on WhatsApp. With no
    // service named, nor one the server takes, the server finds the identity
    // on the phone service first, then WhatsApp.
    const service =
      removeTarget.service === "email" ? undefined : listedServerService(removeTarget.service);
    updateContact.mutate(
      { contactId, body: { remove_identity: { address, service } } },
      { onSuccess: () => setRemoveTarget(null) },
    );
  };

  const requestPickCountry = (target: { address: string; service: string | null }) => {
    if (busy) return;
    updateContact.reset();
    setCountryTarget(target);
  };

  /**
   * Pick the country of the number written without its `+` code. With
   * `merge`, it joins the identity the server named as holding its `+` form.
   */
  const confirmCountry = ({ country, merge }: { country: string; merge: boolean }) => {
    if (!countryTarget || busy) return;
    updateContact.mutate(
      {
        contactId,
        body: {
          set_identity_country: {
            address: countryTarget.address,
            service: listedServerService(countryTarget.service),
            country,
            merge,
          },
        },
      },
      { onSuccess: () => setCountryTarget(null) },
    );
  };

  const confirmAdd = (args: { address: string; service: OfferedService }) => {
    if (busy) return;
    updateContact.mutate(
      {
        contactId,
        body: { add_identity: { address: args.address, service: serverService(args.service) } },
      },
      { onSuccess: () => setAdding(false) },
    );
  };

  return {
    adding,
    setAdding,
    busy,
    error,
    /** The last refusal itself, so the country picker can tell a merge question from a failure. */
    failure: updateContact.error,
    countryTarget,
    setCountryTarget,
    requestPickCountry,
    confirmCountry,
    removeTarget,
    setRemoveTarget,
    requestRemoveHandle,
    confirmRemoveHandle,
    confirmAdd,
  };
}
