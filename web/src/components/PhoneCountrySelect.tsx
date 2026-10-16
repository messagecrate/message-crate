import type { PhoneCountryChoice } from "../lib/phoneCountries";
import { phoneCountryItems } from "./phoneCountryItems";
import Select, { ListBoxItem, selectItemClassName } from "./Select";

/** The choice that states no country. */
const NO_PHONE_COUNTRY = "none";

/**
 * The country a phone number written without its `+` code is read in, with
 * "Not stated" first: the picker the import form's Phone's country and the
 * vCard conversion under Settings both offer. `value` is an ISO code, or ""
 * for none.
 */
export default function PhoneCountrySelect({
  value,
  countries,
  onChange,
  label,
  isDisabled,
  triggerClassName,
}: {
  value: string;
  countries: readonly PhoneCountryChoice[];
  onChange: (code: string) => void;
  /** The accessible name of the control. */
  label: string;
  isDisabled?: boolean;
  triggerClassName?: string;
}) {
  return (
    <Select
      selectedKey={value || NO_PHONE_COUNTRY}
      onSelectionChange={(k) => {
        if (typeof k === "string") onChange(k === NO_PHONE_COUNTRY ? "" : k);
      }}
      aria-label={label}
      isDisabled={isDisabled}
      triggerClassName={triggerClassName}
    >
      {[
        <ListBoxItem
          key={NO_PHONE_COUNTRY}
          id={NO_PHONE_COUNTRY}
          textValue="Not stated"
          className={selectItemClassName}
        >
          Not stated
        </ListBoxItem>,
        ...phoneCountryItems(countries),
      ]}
    </Select>
  );
}
