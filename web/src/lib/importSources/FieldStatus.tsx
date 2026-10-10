import { hintStyle } from "../../screens/import/ImportFormUi";

/** A field's problem under it, read out as it appears; nothing when there is none. */
export function FieldStatus({ message }: { message: string | undefined }) {
  if (!message) return null;
  return (
    <p className={hintStyle} role="status">
      {message}
    </p>
  );
}
