import type { ReactElement } from "react";
import { TimeZoneContext } from "../lib/timeZone";

/**
 * `ui` shown in the account's time zone `zone`.
 *
 * Outside a `TimeZoneContext`, `useTimeZone` falls back to the machine's zone,
 * so a test that checks a day without one passes in UTC, which CI uses, and
 * fails at UTC+12 or later (#1418). The zone is an argument, not a default,
 * so each test names the zone it checks in. `TZ` is not pinned for the run,
 * because a pinned zone would hide the next test that forgets.
 */
export function inTimeZone(zone: string, ui: ReactElement): ReactElement {
  return <TimeZoneContext.Provider value={zone}>{ui}</TimeZoneContext.Provider>;
}
