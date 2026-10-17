import type { components } from "./serverApi.types";

/**
 * One contact in full, as the contact drawer shows it (`useContactDetail` in
 * `components/useContactDetail.ts`) and changes it (`useUpdateContact` in
 * `hooks/useUpdateContact.ts`).
 *
 * `useContactDetailCache` was the last hand-built piece of it: a `Map`, its
 * own in-flight guard, and a `mc-contact-detail-changed` browser event that
 * the drawer subscribed to so that group chips edited in the contact list
 * would show. All three are TanStack Query's now: the cache is keyed by
 * account and contact, and the group chips a contact-list edit writes are the
 * membership mutation's optimistic patch onto this same entry.
 */

export type ContactDetail = components["schemas"]["Contact"];
export type ContactHandle = components["schemas"]["Identity"];
/** One change to a contact: its name, or one identity added, updated or removed. */
export type ContactChange = components["schemas"]["UpdateContactRequest"];
