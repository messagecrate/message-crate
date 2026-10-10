import { ListBoxItem } from "react-aria-components";
import { parseSelectKey } from "../lib/selectKey";
import { EXPORT_FORMATS, type ExportFormat } from "../lib/tauri";
import FormRow from "./FormRow";
import Select, { selectItemClassName } from "./Select";

const FORMAT_IDS = EXPORT_FORMATS.map((f) => f.id);

/** The one name the output format menu carries, on screen and for assistive technology. */
const LABEL = "Output format";

/**
 * The output format menu of the Export screen and of Convert in Settings:
 * one row offering every format in `EXPORT_FORMATS`.
 */
export default function ExportFormatSelect({
  value,
  onChange,
  isDisabled,
}: {
  value: ExportFormat;
  onChange: (format: ExportFormat) => void;
  isDisabled?: boolean;
}) {
  return (
    <FormRow label={LABEL}>
      <Select
        selectedKey={value}
        onSelectionChange={(key) => {
          const next = parseSelectKey(key, FORMAT_IDS);
          if (next) onChange(next);
        }}
        aria-label={LABEL}
        isDisabled={isDisabled}
      >
        {EXPORT_FORMATS.map((option) => (
          <ListBoxItem key={option.id} id={option.id} className={selectItemClassName}>
            {option.label}
          </ListBoxItem>
        ))}
      </Select>
    </FormRow>
  );
}
