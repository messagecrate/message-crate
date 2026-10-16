import { namedPhoneCountries, type PhoneCountryChoice } from "../lib/phoneCountries";
import { keys } from "../lib/queryKeys";
import { listPhoneCountries } from "../lib/serverApi";
import { useRouteQuery } from "./useRouteQuery";

/** The country list, fetched once a session. */
export function usePhoneCountries(): {
  countries: PhoneCountryChoice[];
  loading: boolean;
  error: Error | null;
} {
  const { data, isPending, error } = useRouteQuery(
    keys.phoneCountries.all,
    (signal) => listPhoneCountries({ signal }),
    { staleTime: Number.POSITIVE_INFINITY },
  );
  return {
    countries: data ? namedPhoneCountries(data) : [],
    loading: isPending,
    error: data === undefined ? error : null,
  };
}
