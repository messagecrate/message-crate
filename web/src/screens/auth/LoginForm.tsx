import { type FormEvent, useState } from "react";
import AuthErrorFooter from "../../components/AuthErrorFooter";
import AuthSubmitButton from "../../components/AuthSubmitButton";
import Checkbox from "../../components/Checkbox";
import { setBaseUrl } from "../../lib/api";
import { useAuth } from "../../lib/authContext";
import { login as serverLogin } from "../../lib/serverApi";
import { isTauri } from "../../lib/tauriCheck";
import { useAsyncAction } from "../../lib/useAsyncAction";
import CredentialFields from "./CredentialFields";

/**
 * Username and password login.
 *
 * On the website it offers "Keep me logged in", ticked to start with.
 * Unticked, the login lasts only until the tab closes, for a shared or
 * borrowed computer (#2205). The desktop app does not show the box: it ends
 * the session when its window closes.
 */
export default function LoginForm({
  serverUrl,
  disabled = false,
}: {
  serverUrl: string;
  disabled?: boolean;
}) {
  const { login } = useAuth();
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const offerKeep = !isTauri();
  const [keep, setKeep] = useState(true);
  const { busy, error, run } = useAsyncAction();

  // A real submit, so the browser treats this as a login: Enter in either
  // field submits without a per-field key handler, and a password manager can
  // offer to fill the credentials and to save them afterwards.
  const submit = (event: FormEvent) => {
    event.preventDefault();
    if (busy || disabled) return;
    void run(async () => {
      if (!username.trim()) {
        throw new Error("Username is required.");
      }
      const url = serverUrl.trim();
      // Re-sync the API client with the address this form is showing, the
      // same as `CreateAccountForm` — `connect()` on the login card can
      // leave the client pointed at a bad host while the form still holds
      // the good address.
      setBaseUrl(url);
      const res = await serverLogin({ username, password });
      // Awaited so the profile lookup inside `login` has decided where to send
      // the user before this form drops its busy state.
      await login(url, res.token, res.account_id, !offerKeep || keep);
    });
  };

  // Beside the box on the website, on its own row in the desktop app.
  const submitButton = (className?: string) => (
    <AuthSubmitButton disabled={busy || disabled} className={className}>
      {busy ? "Logging in…" : "Log in"}
    </AuthSubmitButton>
  );

  return (
    <form className="flex min-h-0 flex-1 flex-col" onSubmit={submit}>
      <CredentialFields
        username={{ value: username, onChange: setUsername }}
        password={{ value: password, onChange: setPassword }}
        autoComplete="current-password"
        disabled={disabled}
      />

      {offerKeep ? (
        <div className="mt-5 flex items-center justify-between gap-3">
          <Checkbox checked={keep} onChange={setKeep} disabled={busy || disabled}>
            Keep me logged in
          </Checkbox>
          {submitButton("min-w-[50%]")}
        </div>
      ) : (
        submitButton()
      )}

      {/* Pushed to the foot of the panel so the message lands just above the
          rule that closes the card, clear of the action that produced it. The
          band is taller than the default because the space above it is empty
          anyway, and a message that wraps grows up into it. */}
      <div className="mt-auto">
        <AuthErrorFooter error={error} className="h-16" />
      </div>
    </form>
  );
}
