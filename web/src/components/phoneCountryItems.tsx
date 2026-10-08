import type { ReactElement } from "react";
import { type PhoneCountryChoice, phoneCountryLabel } from "../lib/phoneCountries";
import { ListBoxItem, selectItemClassName } from "./Select";

/**
 * One `Select` item per country, labelled with its name and calling code:
 * the list the import form's Phone's country and the Pick country dialog
 * both offer.
 */
export function phoneCountryItems(countries: readonly PhoneCountryChoice[]): ReactElement[] {
  return countries.map((c) => (
    <ListBoxItem key={c.code} id={c.code} textValue={c.name} className={selectItemClassName}>
      {phoneCountryLabel(c)}
    </ListBoxItem>
  ));
}
