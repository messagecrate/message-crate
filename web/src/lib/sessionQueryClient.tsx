import { QueryClientProvider } from "@tanstack/react-query";
import { type ReactNode, useState } from "react";
import { createQueryClient } from "./routeQuery";
import { currentSessionRefused } from "./sessionRefusal";

/**
 * The query client every screen fetches through, built once for the life of
 * the app.
 *
 * A query or mutation the server refuses because the session has ended has
 * to log the person out, and the client only learns how at construction. It
 * reports the refusal through `sessionRefusal.ts`, where `AuthProvider`
 * registers the logout, because `AuthProvider` sits inside this provider: it
 * reads the client to clear the cache and to fetch the profile.
 */
export function SessionQueryClientProvider({ children }: { children: ReactNode }) {
  const [queryClient] = useState(() =>
    createQueryClient({ onUnauthorized: currentSessionRefused }),
  );
  return <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>;
}
