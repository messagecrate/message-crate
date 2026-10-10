import { useState } from "react";
import { LockIcon, PersonIcon } from "../../components/icons";
import PasswordField from "../../components/PasswordField";
import TextField from "../../components/TextField";

/**
 * The username and password fields of the three sign-in forms: Login, Create
 * Account, and Create Owner.
 *
 * `autoComplete` says which password this is. `current-password` is a login,
 * so a password manager offers to fill it; `new-password` is one being chosen,
 * so a password manager offers to generate and store it. The Confirm Password
 * field shows only when the form passes `confirmPassword`. Each password field
 * keeps its own show/hide state, so showing one does not show the other.
 */
export default function CredentialFields({
  username,
  onUsernameChange,
  password,
  onPasswordChange,
  autoComplete,
  confirmPassword,
  disabled = false,
}: {
  username: string;
  onUsernameChange: (value: string) => void;
  password: string;
  onPasswordChange: (value: string) => void;
  autoComplete: "current-password" | "new-password";
  /** The Confirm Password field's value and setter; leave out for no field. */
  confirmPassword?: { value: string; onChange: (value: string) => void };
  disabled?: boolean;
}) {
  const [showPassword, setShowPassword] = useState(false);
  const [showConfirm, setShowConfirm] = useState(false);

  return (
    <>
      <TextField
        label="Username"
        leadingIcon={<PersonIcon size={16} />}
        value={username}
        onChange={onUsernameChange}
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
        value={password}
        onChange={onPasswordChange}
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
