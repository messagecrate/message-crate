import { serverName } from "../../lib/serverName";

/**
 * How the auth card is getting on with the server it resolved.
 *
 * `untested` is the settings screen's own state and never the card's: an
 * address that has been typed but not tried yet. It is a fourth answer rather
 * than a shade of the other three, because "we have not asked" is not the same
 * as reaching a server, failing to reach one, or being part-way through.
 */
export type ServerConnection = "connecting" | "connected" | "disconnected" | "untested";

/** Each state's word, and what joins it to the server's name. */
const WORDING: Record<ServerConnection, { word: string; join: string }> = {
  connecting: { word: "Connecting", join: " to " },
  connected: { word: "Connected", join: " to " },
  disconnected: { word: "Disconnected", join: " from " },
  untested: { word: "Not tested", join: ": " },
};

/** The state's word, joined to the server's name when there is one. */
function sentence(state: ServerConnection, address: string): string {
  const { word, join } = WORDING[state];
  const name = serverName(address);
  return name === "" ? word : `${word}${join}${name}`;
}

/**
 * The words carry the state on their own — there is no indicator dot. Connecting
 * keeps the slow flash the old light had, moved onto the text as an opacity
 * pulse, because scaling type wobbles the baseline underneath it.
 *
 * Untested is grey and still: green and red are answers about a server, and an
 * address nobody has tried has not earned either one. It does not pulse, since
 * nothing is happening for the pulse to stand for.
 */
const TONE: Record<ServerConnection, string> = {
  connecting: "text-text motion-safe:animate-pulse",
  connected: "text-ok",
  disconnected: "text-danger",
  untested: "text-muted",
};

export interface ServerStatusProps {
  state: ServerConnection;
  /**
   * The address the state is about, as the person entered it or the card
   * resolved it. Blank when there is no address to name, as in an emptied
   * Address field: the state's word then stands alone.
   */
  address: string;
  /**
   * Words to show in place of the state's own. The desktop app uses it while
   * it starts its own Message Crate: the state is still "connecting", and
   * what the person is waiting for has a better name than that.
   */
  label?: string;
  className?: string;
}

/**
 * The server connection, naming the server: "Connected to localhost:8080".
 * The name is there because the desktop app can be pointed at its own Message
 * Crate or at one on another computer, and the card is where a person sees
 * which one they are about to log in to (#1973). `m-0` because theme.css
 * leaves out Tailwind's preflight, so a paragraph otherwise carries the
 * browser's own margins and floats away from the line it belongs under; the
 * gap below is the caller's to set.
 */
export default function ServerStatus({ state, address, label, className }: ServerStatusProps) {
  return (
    <p
      role="status"
      className={`m-0 text-[0.813rem] font-medium ${TONE[state]} ${className ?? ""}`}
    >
      {label ?? sentence(state, address)}
    </p>
  );
}
