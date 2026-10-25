/**
 * The countries a phone number written without its `+` code can be read in,
 * from the server's own table (`GET /v1/phone-countries`), each named in the
 * browser's language.
 *
 * A number whose country nobody stated is stored as the digits typed and
 * matches no number written with its `+` (#1676). The import form's Default
 * country for contacts states one for a whole run, and a vCard file
 * conversion for the whole file. The Contacts screen, My Identities, and the
 * Profile tab of an account the administrator opens from User Accounts each pick one
 * for a single identity. Each of them offers this list.
 */

import type { components } from "./serverApi.types";

/** The label of the import form's and the vCard file conversion's country field. */
export const DEFAULT_COUNTRY_LABEL = "Default country for contacts";

/** The help text under Default country for contacts, on the import form and the vCard file conversion. */
export const DEFAULT_COUNTRY_HINT =
  "Applied to phone numbers that don't include a country code: usually the country of the phone's own number. Leave empty to pick each number's country later, on the contact.";

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

/**
 * The countries a typed search finds, in the list's order: by name (`ger`),
 * by calling code with or without its `+` (`+49`, `49`), or by ISO code
 * (`DE`). A calling code matches from its start, so `4` finds every code
 * that starts with 4. The country whose ISO code was typed comes first,
 * because `de` is also in the names of Bangladesh and Sweden. Nothing typed
 * finds every country.
 */
export function searchPhoneCountries(
  countries: readonly PhoneCountryChoice[],
  query: string,
): PhoneCountryChoice[] {
  const q = query.trim().toLocaleLowerCase();
  if (q === "") return [...countries];
  const digits = /^\+?\d+$/.test(q) ? q.replace(/^\+/, "") : null;
  const byCode = countries.filter((c) => c.code.toLowerCase() === q);
  const rest = countries.filter(
    (c) =>
      c.code.toLowerCase() !== q &&
      (c.name.toLocaleLowerCase().includes(q) ||
        (digits !== null && c.calling_code.startsWith(digits))),
  );
  return [...byCode, ...rest];
}
