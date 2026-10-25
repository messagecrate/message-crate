import LoginForm from "./LoginForm";
import SetupForm from "./SetupForm";
import type { ServerState } from "./useServerState";

/**
 * The way into a Message Crate, which depends on what state it is in.
 *
 * A Message Crate in **setup** offers one thing: setting the password of
 * `admin`, the administrator, who is made with the database. No login,
 * because no account has a password to log in with yet.
 *
 * A **ready** Message Crate offers Login alone. Accounts are created by the
 * administrator, so there is no Create Account here.
 *
 * The server reports which of the two it is; nothing here recombines the
 * facts behind that answer. See
 * `docs/adr/0008-the-administrator-holds-no-messages.md`.
 */
export default function LocalAuthTabs({
  serverUrl,
  serverState,
  disabled = false,
}: {
  serverUrl: string;
  serverState: ServerState;
  disabled?: boolean;
}) {
  const setup = serverState === "setup";
  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <h2 className="mb-6 border-b border-border pb-2 text-center text-[0.875rem] font-medium text-text">
        {setup ? "Set Up" : "Login"}
      </h2>
      {setup ? (
        <SetupForm serverUrl={serverUrl} disabled={disabled} />
      ) : (
        <LoginForm serverUrl={serverUrl} disabled={disabled} />
      )}
    </div>
  );
}
