import { useContext } from "react";
import { browserTimeZone, TimeZoneContext } from "../lib/timeZone";

/** The zone to show message times in. Outside a provider: the browser's. */
export function useTimeZone(): string {
  return useContext(TimeZoneContext) ?? browserTimeZone();
}
