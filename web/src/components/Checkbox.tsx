import { type ReactNode, useRef } from "react";
import { mergeProps } from "react-aria/mergeProps";
import { useCheckbox } from "react-aria/useCheckbox";
import { useFocusRing } from "react-aria/useFocusRing";
import { useToggleState } from "react-stately/useToggleState";

/**
 * The app's checkbox: React Aria's `useCheckbox`, drawn as the compact list
 * box (`.mc-list-check` in `theme.css`).
 *
 * The `<input>` lies over the drawn box, the same size and see-through, so a
 * press on the box lands on the input itself, and a browser test or an
 * assistive tool can target the one element that works (#3251). React Aria's
 * `Checkbox` component hides its input in a clipped 1px span instead, where a
 * press lands on the drawn box or the label. The input never shows a focus
 * ring of its own, so the label carries the state as marks the box draws
 * from: `data-selected`, `data-indeterminate`, `data-disabled` and, for the
 * focus ring, `data-focus-visible`.
 *
 * Pass `children` for a visible label; otherwise `aria-label` is required, since
 * a checkbox with neither announces as an unnamed control.
 */
type CheckboxProps = {
  checked: boolean;
  /** Mixed state — some but not all of the things this box covers are checked. */
  indeterminate?: boolean;
  /** `shiftKey` is there for a caller that checks a range on Shift + click. */
  onChange: (checked: boolean, modifiers: { shiftKey: boolean }) => void;
  disabled?: boolean;
  /** Extra classes for the drawn box, with the input over it. */
  className?: string;
  /** Extra classes for the label around the box (and `children`, when given). */
  labelClassName?: string;
  children?: ReactNode;
} & ({ children: ReactNode } | { "aria-label": string });

export default function Checkbox({
  checked,
  indeterminate = false,
  onChange,
  disabled,
  className = "",
  labelClassName = "",
  children,
  ...rest
}: CheckboxProps) {
  // React Aria's onChange carries no event, so the Shift key is read when the press starts.
  const shiftRef = useRef(false);
  const inputRef = useRef<HTMLInputElement>(null);
  const isIndeterminate = indeterminate && !checked;
  const ariaProps = {
    ...rest,
    children,
    isSelected: checked,
    isIndeterminate,
    isDisabled: disabled,
    onPressStart: (e: { shiftKey: boolean }) => {
      shiftRef.current = e.shiftKey;
    },
    onChange: (next: boolean) => {
      onChange(next, { shiftKey: shiftRef.current });
      shiftRef.current = false;
    },
  };
  const state = useToggleState(ariaProps);
  const { labelProps, inputProps, isDisabled } = useCheckbox(ariaProps, state, inputRef);
  const { focusProps, isFocusVisible } = useFocusRing();
  const labelClass =
    children === undefined
      ? "inline-flex cursor-pointer data-disabled:cursor-not-allowed"
      : "inline-flex cursor-pointer items-center gap-2 text-[0.813rem] text-text data-disabled:cursor-not-allowed";

  return (
    <label
      {...labelProps}
      className={`mc-check ${labelClass} ${labelClassName}`}
      data-selected={state.isSelected || undefined}
      data-indeterminate={isIndeterminate || undefined}
      data-disabled={isDisabled || undefined}
      data-focus-visible={isFocusVisible || undefined}
    >
      <span className={`mc-list-check-frame shrink-0 ${className}`}>
        <input {...mergeProps(inputProps, focusProps)} ref={inputRef} className="mc-check-input" />
        <span aria-hidden className="mc-list-check" />
      </span>
      {children}
    </label>
  );
}
