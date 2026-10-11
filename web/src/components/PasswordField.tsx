import type { ReactNode } from "react";
import { Input, Label, TextField, type TextFieldProps } from "react-aria-components";
import Button from "./Button";
import { EyeIcon, EyeOffIcon } from "./icons";
import { leadingIconClass, textInputClass } from "./TextField";

/** Masked password input matching shared form chrome, with show/hide toggle. */
export default function PasswordField(
  props: TextFieldProps & {
    label?: string;
    /** Optional glyph rendered inside the field, before the text. */
    leadingIcon?: ReactNode;
    showPassword: boolean;
    onToggle: () => void;
  },
) {
  const { label, leadingIcon, showPassword, onToggle, className, ...rest } = props;
  return (
    <TextField {...rest} className={`block w-full ${className ?? ""}`}>
      {label ? (
        <Label className="mb-1 block text-[0.875rem] font-medium text-text">{label}</Label>
      ) : null}
      <div className="relative">
        {leadingIcon ? <span className={leadingIconClass}>{leadingIcon}</span> : null}
        <Input
          type={showPassword ? "text" : "password"}
          className={`${textInputClass} pr-11 ${leadingIcon ? "pl-10" : ""}`}
        />
        <Button
          variant="ghost"
          onPress={onToggle}
          className="!absolute top-1/2 right-1.5 !-translate-y-1/2 !border-none !p-1.5 text-muted hover:text-text"
          aria-label={showPassword ? "Hide password" : "Show password"}
        >
          {showPassword ? (
            <EyeOffIcon size={16} className="" />
          ) : (
            <EyeIcon size={16} className="" />
          )}
        </Button>
      </div>
    </TextField>
  );
}
