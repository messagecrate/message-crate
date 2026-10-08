/**
 * The countries a phone number written without its `+` code can be read in,
 * from the server's own table (`GET /v1/phone-countries`), each named in the
 * browser's language.
 *
 * A number whose country nobody stated is stored as the digits typed and
 * matches no number written with its `+` (#1676). The import form states the
 * phone's country for a whole run, and the Contacts and My Identities screens
 * pick one for a single identity; both offer this list.
 */

import { keys } from "./queryKeys";
import { useRouteQuery } from "./routeQuery";
import { listPhoneCountries } from "./serverApi";
import type { components } from "./serverApi.types";

/** One country, with the name the person reads it by. */
export type PhoneCountryChoice = components["schemas"]["PhoneCountry"] & {
  /** The country's name in the browser's language, such as "United Kingdom". */
  name: string;
};

/** The label one choice shows: its name and its calling code, "United Kingdom (+44)". */
export function phoneCountryLabel(country: PhoneCountryChoice): string {
  return `${country.name} (+${country.calling_code})`;
}

/**
 * The countries named in `locale` and sorted by name. A code the browser
 * cannot name keeps the code as its name.
 */
export function namedPhoneCountries(
  countries: readonly components["schemas"]["PhoneCountry"][],
  locale?: string,
): PhoneCountryChoice[] {
  let names: Intl.DisplayNames | null = null;
  try {
    names = new Intl.DisplayNames(locale ? [locale] : undefined, { type: "region" });
  } catch {
    names = null;
  }
  return countries
    .map((country) => ({ ...country, name: names?.of(country.code) ?? country.code }))
    .sort((a, b) => a.name.localeCompare(b.name, locale));
}

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
