import { Navigate, Outlet } from "react-router-dom";
import { useAccountProfile } from "../hooks/useAccountProfile";
import { useAuth } from "../hooks/useAuth";
import { useIsAdmin } from "../hooks/useIsAdmin";
import { useNeedsProfileSetup } from "../hooks/useNeedsProfileSetup";
import Button from "./Button";
import LocalServerNotice from "./LocalServerNotice";

/**
 * Layout route: renders child routes via <Outlet /> when the account may use
 * the app, and otherwise sends it to the one screen it still owes.
 */
export function AuthGuard() {
  const { isAuthenticated } = useAuth();
  const { isAdmin, error: adminError } = useIsAdmin();
  const { needsSetup, loading, error: setupError } = useNeedsProfileSetup();

  if (!isAuthenticated) {
    return <Navigate to="/login" replace />;
  }

  // The profile is fetched during login, so this is over before it is seen.
  // Rendering the app first and redirecting after would flash a screen this
  // account is not finished earning.
  if (loading) {
    return null;
  }

  // With no profile, neither the administrator nor an account that owes its setup can
  // be told apart from an account that owes nothing, so none of the screens
  // below may render.
  const error = adminError || setupError;
  if (error) {
    return <ProfileLoadFailed error={error} />;
  }

  // The administrator holds no messages, so every route under this guard is empty for
  // them. Admin Home is the whole of what they have.
  if (isAdmin) {
    return <Navigate to="/admin" replace />;
  }

  if (needsSetup) {
    return <Navigate to="/onboarding" replace />;
  }

  return <Outlet />;
}

/** In place of the app while the account's profile cannot be loaded. */
function ProfileLoadFailed({ error }: { error: string }) {
  const { retry } = useAccountProfile();
  return (
    <div className="flex h-screen flex-col bg-bg font-sans text-text">
      {/* The likeliest reason a profile cannot be loaded in the desktop app
          is that its own server stopped, so the notice that says so, and
          starts it again, shows here too. */}
      <LocalServerNotice />
      <div className="flex min-h-0 flex-1 flex-col items-center justify-center gap-4 p-6">
        <div className="max-w-md text-center" role="alert">
          <p className="m-0 font-semibold text-[0.938rem]">Your profile could not be loaded.</p>
          <p className="m-0 mt-1 text-[0.813rem] text-muted">{error}</p>
        </div>
        <Button variant="primary" onPress={retry}>
          Try again
        </Button>
      </div>
    </div>
  );
}
