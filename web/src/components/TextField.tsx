import type { InputHTMLAttributes, KeyboardEventHandler, ReactNode } from "react";
import {
  FieldError,
  Input,
  Label,
  TextField as RACTextField,
  type TextFieldProps as RACTextFieldProps,
  Text,
} from "react-aria-components";

/** Shared chrome for text inputs (settings, forms, PathPicker, etc.). */
export const textInputClass =
  "box-border w-full rounded-xl border border-border bg-bg px-3 py-2.5 text-[0.875rem] text-text outline-none focus:border-accent disabled:opacity-50";

/**
 * Position of a decorative glyph inside a field, left of the text. `flex` is
 * what centres it: as an inline span the box is as tall as the line box, not
 * as the glyph, so the icon inside it sat low against the field's own text.
 * Laying the span out as a flex box shrinks it to the glyph, and only then
 * does centring on the field's midpoint put the glyph there.
 */
export const leadingIconClass =
  "pointer-events-none absolute top-1/2 left-3 flex -translate-y-1/2 items-center text-muted";

/**
 * Shared text input wrapping React Aria's TextField + Input.
 *
 * `label` renders an internal Label; `hint` renders a description slot.
 * `leadingIcon` puts a glyph inside the field and moves the text clear of it.
 * React Aria's TextField type omits some input-level props (placeholder,
 * onKeyDown, ...) even though it forwards them at runtime — re-declare them
 * so callers can pass them straight through.
 */
export interface TextFieldProps extends RACTextFieldProps {
  label?: string;
  /** Optional control beside the label (e.g. a status light). */
  labelEnd?: ReactNode;
  /** Optional glyph rendered inside the field, before the text. */
  leadingIcon?: ReactNode;
  hint?: string;
  inputClassName?: string;
  className?: string;
  placeholder?: string;
  autoComplete?: string;
  autoFocus?: boolean;
  type?: string;
  name?: string;
  inputMode?: InputHTMLAttributes<HTMLInputElement>["inputMode"];
  onKeyDown?: KeyboardEventHandler<HTMLInputElement>;
  maxLength?: number;
  minLength?: number;
  pattern?: string;
}

export default function TextField({
  label,
  labelEnd,
  leadingIcon,
  hint,
  inputClassName,
  className,
  ...props
}: TextFieldProps) {
  const input = (
    <Input className={`${textInputClass} ${leadingIcon ? "pl-10" : ""} ${inputClassName ?? ""}`} />
  );

  return (
    <RACTextField {...props} className={className}>
      {label ? (
        <div className="mb-1 flex items-center gap-2">
          <Label className="text-[0.875rem] font-medium text-text">{label}</Label>
          {labelEnd}
        </div>
      ) : null}
      {leadingIcon ? (
        <div className="relative">
          <span className={leadingIconClass}>{leadingIcon}</span>
          {input}
        </div>
      ) : (
        input
      )}
      {hint && (
        <Text slot="description" className="mt-1 block text-[0.75rem] text-muted">
          {hint}
        </Text>
      )}
      <FieldError className="mt-1 block text-[0.75rem] text-danger" />
    </RACTextField>
  );
}
