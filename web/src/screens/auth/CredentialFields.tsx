import { useState } from "react";
import { LockIcon, PersonIcon } from "../../components/icons";
import PasswordField from "../../components/PasswordField";
import TextField from "../../components/TextField";

/** One field's value, and the setter the form keeps it with. */
type FieldState = { value: string; onChange: (value: string) => void };

/**
 * The username and password fields of the forms that sign in or create an
 * account: Login, Set Up, and the administrator's New Account. Set Up passes
 * no `username`, because the administrator's username is fixed, and shows no
 * Username field.
 *
 * `autoComplete` says which password this is. `current-password` is a login,
 * so a password manager offers to fill it. `new-password` is one being chosen,
 * so a password manager offers to generate and store it. Each password field
 * keeps its own show/hide state, so showing one does not show the other.
 */
export default function CredentialFields({
  username,
  password,
  confirmPassword,
  autoComplete,
  disabled = false,
}: {
  /** Without it, the form shows no Username field. */
  username?: FieldState;
  password: FieldState;
  /** Without it, the form shows no Confirm Password field. */
  confirmPassword?: FieldState;
  autoComplete: "current-password" | "new-password";
  disabled?: boolean;
}) {
  const [showPassword, setShowPassword] = useState(false);
  const [showConfirm, setShowConfirm] = useState(false);

  return (
    <>
      {username ? (
        <TextField
          label="Username"
          leadingIcon={<PersonIcon size={16} className="shrink-0" />}
          value={username.value}
          onChange={username.onChange}
          name="username"
          autoComplete="username"
          isDisabled={disabled}
        />
      ) : null}

      <PasswordField
        label="Password"
        className={username ? "mt-3.5" : undefined}
        leadingIcon={<LockIcon size={16} className="shrink-0" />}
        value={password.value}
        onChange={password.onChange}
        name={autoComplete === "current-password" ? "password" : "new-password"}
        autoComplete={autoComplete}
        showPassword={showPassword}
        onToggle={() => setShowPassword((v) => !v)}
        isDisabled={disabled}
      />

      {confirmPassword ? (
        <PasswordField
          label="Confirm Password"
          className="mt-3.5"
          leadingIcon={<LockIcon size={16} className="shrink-0" />}
          value={confirmPassword.value}
          onChange={confirmPassword.onChange}
          name="confirm-password"
          autoComplete="new-password"
          showPassword={showConfirm}
          onToggle={() => setShowConfirm((v) => !v)}
          isDisabled={disabled}
        />
      ) : null}
    </>
  );
}
