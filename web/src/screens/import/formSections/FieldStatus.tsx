import { hintClass } from "../ImportFormUi";

/** A field's problem under it, read out as it appears; nothing when there is none. */
export function FieldStatus({ message }: { message: string | undefined }) {
  if (!message) return null;
  return (
    <p className={hintClass} role="status">
      {message}
    </p>
  );
}
