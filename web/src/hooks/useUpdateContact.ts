import type { UseMutationResult } from "@tanstack/react-query";
import type { ContactChange, ContactDetail } from "../lib/contactDetail";
import { keys } from "../lib/queryKeys";
import { updateContact } from "../lib/serverApi";
import { useRouteCache, useRouteMutation } from "./useRouteQuery";

/**
 * Change one thing about a contact.
 *
 * The server answers with the contact as it now stands, so the answer goes
 * straight into the entry the drawer reads, and the drawer shows it before
 * anything is fetched again. The contact list and every conversation that
 * names the contact show the old name until the account's cache, marked stale
 * once the write settles, is fetched again.
 */
export function useUpdateContact(): UseMutationResult<
  ContactDetail,
  Error,
  { contactId: string; body: ContactChange }
> {
  const cache = useRouteCache();
  return useRouteMutation<ContactDetail, Error, { contactId: string; body: ContactChange }>({
    mutationFn: ({ contactId, body }) => updateContact(contactId, body),
    onSuccess: (detail, { contactId }) => {
      cache.set(keys.contacts.detail(contactId), detail);
    },
  });
}
