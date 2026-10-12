import type { ReactNode } from "react";
import { AuthProvider } from "../lib/AuthProvider";
import LogoutDialog from "./LogoutDialog";

/**
 * The login state with logout's dialog beside what it wraps. `App.tsx`
 * renders the app inside it, and the logout tests render through it, so the
 * dialog the tests see is the one the app places.
 */
export default function AuthShell({ children }: { children: ReactNode }) {
  return (
    <AuthProvider>
      {children}
      <LogoutDialog />
    </AuthProvider>
  );
}
