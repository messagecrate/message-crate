import { useRouteQuery } from "../hooks/useRouteQuery";
import type { ContactDetail } from "../lib/contactDetail";
import { keys } from "../lib/queryKeys";
import { getContact } from "../lib/serverApi";

/**
 * The contact behind an open drawer. Skipped entirely when no contact is open.
 *
 * `error` is why the last load failed, and `retry` asks again. Without them a
 * drawer whose contact could not be loaded stayed on "Loading…" for good.
 */
export function useContactDetail(contactId: string | null): {
  detail: ContactDetail | null;
  loading: boolean;
  error: Error | null;
  retry: () => void;
} {
  const { data, isPending, error, refetch } = useRouteQuery(
    keys.contacts.detail(contactId ?? ""),
    (signal) => getContact(contactId ?? "", { signal }),
    { enabled: contactId !== null },
  );
  return {
    detail: contactId ? (data ?? null) : null,
    loading: isPending,
    error: contactId ? error : null,
    retry: () => {
      void refetch();
    },
  };
}
