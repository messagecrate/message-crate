import { useEffect, useMemo, useState } from "react";
import { Button, ComboBox, Input, ListBox, ListBoxItem, Popover } from "react-aria-components";
import {
  type PhoneCountryChoice,
  phoneCountryLabel,
  searchPhoneCountries,
} from "../lib/phoneCountries";
import { popupShadowClass } from "../lib/uiStyles";
import { Z_POPOVER } from "../lib/zLayers";
import { SelectChevronIcon } from "./icons";
import { selectItemClassName } from "./Select";
import { textInputClass } from "./TextField";

/** The key of the row that states no country. */
const NONE_ROW = "none";

/** The label of the row that states no country, and the empty field's placeholder. */
export const NO_COUNTRY_LABEL = "None";

/**
 * The country a phone number written without its `+` code is read in, as a
 * search box over the list: the import form's Default country for contacts,
 * the vCard file conversion's under Settings, and the Pick country dialog's.
 * Typing narrows the rows by name, calling code or ISO code
 * (`searchPhoneCountries`); leaving without picking puts back the field's
 * choice. Built like `TimeZoneField`.
 *
 * `value` is an ISO code, or "" for none. With `allowsNone`, a `None` row
 * comes first and gives "". Without it, as in the Pick country dialog, which
 * sets one number's country, there is no row to state none.
 */
export default function CountryField({
  value,
  countries,
  onChange,
  label,
  id,
  allowsNone = false,
  placeholder,
  isDisabled,
  className,
  inputClassName,
  popoverClassName = Z_POPOVER,
}: {
  value: string;
  countries: readonly PhoneCountryChoice[];
  onChange: (code: string) => void;
  /** The accessible name of the field. */
  label: string;
  /** The search box's id, for a `<label>` beside the field. */
  id?: string;
  allowsNone?: boolean;
  /** What the empty field shows; `None` when the field allows none. */
  placeholder?: string;
  isDisabled?: boolean;
  className?: string;
  inputClassName?: string;
  /** The popover's stacking class: a dialog's field needs the one above modals. */
  popoverClassName?: string;
}) {
  const selected = useMemo(
    () => countries.find((c) => c.code === value) ?? null,
    [countries, value],
  );
  const selectedLabel = selected ? phoneCountryLabel(selected) : "";
  const [text, setText] = useState(selectedLabel);

  useEffect(() => {
    setText(selectedLabel);
  }, [selectedLabel]);

  // The picked row's own label in the field is not a search.
  const query = text === selectedLabel ? "" : text;
  const rows = useMemo(() => searchPhoneCountries(countries, query), [countries, query]);
  const showsNone = allowsNone && query === "";
  // A stored code stays the key while the list loads, so the field never
  // reads as None for a country that is picked.
  const selectedKey = value !== "" ? value : allowsNone ? NONE_ROW : null;

  return (
    <ComboBox
      aria-label={label}
      className={className}
      isDisabled={isDisabled}
      menuTrigger="focus"
      allowsEmptyCollection
      // The rows below are already narrowed by `searchPhoneCountries`, which
      // matches on more than the label a row shows.
      defaultFilter={() => true}
      selectedKey={selectedKey}
      inputValue={text}
      onInputChange={setText}
      onSelectionChange={(key) => {
        // Emptying the field reports null: not a pick, the choice stays.
        if (typeof key !== "string") return;
        const code = key === NONE_ROW ? "" : key;
        const picked = countries.find((c) => c.code === code);
        setText(picked ? phoneCountryLabel(picked) : "");
        if (code !== value) onChange(code);
      }}
      onOpenChange={(open) => {
        if (!open) setText(selectedLabel);
      }}
    >
      <div className="relative">
        <Input
          id={id}
          className={`${textInputClass} pr-9 ${inputClassName ?? ""}`}
          placeholder={placeholder ?? (allowsNone ? NO_COUNTRY_LABEL : undefined)}
          // Typing replaces the picked row's label.
          onFocus={(e) => {
            if (selectedLabel !== "" && e.currentTarget.value === selectedLabel) {
              e.currentTarget.select();
            }
          }}
        />
        <Button className="absolute inset-y-0 right-0 flex w-9 items-center justify-center border-0 bg-transparent text-muted outline-none">
          <SelectChevronIcon size={10} />
        </Button>
      </div>
      <Popover
        data-mc-overlay=""
        className={`box-border w-[var(--trigger-width)] rounded-md border border-border bg-popover p-1 outline-none ${popoverClassName} ${popupShadowClass}`}
      >
        <ListBox
          className="max-h-72 overflow-auto outline-none"
          renderEmptyState={() => (
            <div className="px-2 py-1 text-[0.875rem] text-muted">No country matches</div>
          )}
        >
          {showsNone && (
            <ListBoxItem id={NONE_ROW} textValue={NO_COUNTRY_LABEL} className={selectItemClassName}>
              {NO_COUNTRY_LABEL}
            </ListBoxItem>
          )}
          {rows.map((c) => (
            <ListBoxItem
              key={c.code}
              id={c.code}
              textValue={phoneCountryLabel(c)}
              className={selectItemClassName}
            >
              {phoneCountryLabel(c)}
            </ListBoxItem>
          ))}
        </ListBox>
      </Popover>
    </ComboBox>
  );
}
