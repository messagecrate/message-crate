import { type FormEvent, useState } from "react";
import AuthErrorFooter from "../../components/AuthErrorFooter";
import { useAsyncAction } from "../../hooks/useAsyncAction";
import { useAuth } from "../../hooks/useAuth";
import { setBaseUrl } from "../../lib/api";
import { requireMatchingPasswords } from "../../lib/passwordConfirmation";
import { setUpServer } from "../../lib/serverApi";
import CredentialFields from "./CredentialFields";
import KeepLoggedInSubmit from "./KeepLoggedInSubmit";
import { useKeepLoggedIn } from "./useKeepLoggedIn";

/**
 * Set the administrator's password, which is the only thing a Message Crate
 * in setup offers.
 *
 * The administrator is `admin`, made with the database, so there is no
 * username to choose: this form asks for the password twice and nothing
 * else. Submitting sets it and logs the person in as `admin`, who lands on
 * Admin Home.
 */
export default function SetupForm({
  serverUrl,
  disabled = false,
}: {
  serverUrl: string;
  disabled?: boolean;
}) {
  const { login } = useAuth();
  const [password, setPassword] = useState("");
  const [confirmPassword, setConfirmPassword] = useState("");
  const keep = useKeepLoggedIn();
  const { busy, error, run } = useAsyncAction();

  const submit = (event: FormEvent) => {
    event.preventDefault();
    if (busy || disabled) return;
    void run(async () => {
      requireMatchingPasswords(password, confirmPassword);
      const url = serverUrl.trim();
      setBaseUrl(url);
      const res = await setUpServer({ password });
      await login(url, res.token, res.account_id, keep.lifetime);
    });
  };

  return (
    <form className="flex min-h-0 flex-1 flex-col" onSubmit={submit}>
      <p className="mb-5 text-[0.875rem] leading-relaxed text-muted">
        Choose the password for admin, the administrator who creates and manages accounts.
      </p>
      <CredentialFields
        password={{ value: password, onChange: setPassword }}
        autoComplete="new-password"
        confirmPassword={{ value: confirmPassword, onChange: setConfirmPassword }}
        disabled={disabled}
      />
      <KeepLoggedInSubmit choice={keep} disabled={busy || disabled}>
        {busy ? "Saving…" : "Continue"}
      </KeepLoggedInSubmit>
      <div className="mt-auto">
        <AuthErrorFooter error={error} className="h-16" />
      </div>
    </form>
  );
}
