import { useState } from "react";
import { LockIcon, PersonIcon } from "../../components/icons";
import PasswordField from "../../components/PasswordField";
import TextField from "../../components/TextField";

/** One field's value, and the setter the form keeps it with. */
type Field = { value: string; onChange: (value: string) => void };

/**
 * The username and password fields of the three sign-in forms: Login, Create
 * Account, and Create Owner.
 *
 * `autoComplete` says which password this is. `current-password` is a login,
 * so a password manager offers to fill it. `new-password` is one being chosen,
 * so a password manager offers to generate and store it. The Confirm Password
 * field shows only when the form passes `confirmPassword`. Each password field
 * keeps its own show/hide state, so showing one does not show the other.
 */
export default function CredentialFields({
  username,
  password,
  confirmPassword,
  autoComplete,
  disabled = false,
}: {
  username: Field;
  password: Field;
  /** Without it, the form shows no Confirm Password field. */
  confirmPassword?: Field;
  autoComplete: "current-password" | "new-password";
  disabled?: boolean;
}) {
  const [showPassword, setShowPassword] = useState(false);
  const [showConfirm, setShowConfirm] = useState(false);

  return (
    <>
      <TextField
        label="Username"
        leadingIcon={<PersonIcon size={16} />}
        value={username.value}
        onChange={username.onChange}
        name="username"
        autoComplete="username"
        isDisabled={disabled}
      />

      {/* The same gap on every form, so the field does not shift under the
          pointer when the Login and Create Account tabs are switched. */}
      <PasswordField
        label="Password"
        className="mt-3.5"
        leadingIcon={<LockIcon size={16} />}
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
          leadingIcon={<LockIcon size={16} />}
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
