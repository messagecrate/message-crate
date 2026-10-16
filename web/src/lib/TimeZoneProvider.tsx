import type { ReactNode } from "react";
import { useAccountProfile } from "../hooks/useAccountProfile";
import { browserTimeZone, knowsTimeZone, TimeZoneContext } from "./timeZone";

/**
 * Hands the account's zone to everything under it. Until the profile has
 * loaded, or when it names a zone this browser cannot format, the browser's
 * own zone stands in.
 */
export function TimeZoneProvider({ children }: { children: ReactNode }) {
  const { profile } = useAccountProfile();
  const saved = profile?.time_zone;
  const zone = saved && knowsTimeZone(saved) ? saved : browserTimeZone();
  return <TimeZoneContext.Provider value={zone}>{children}</TimeZoneContext.Provider>;
}
