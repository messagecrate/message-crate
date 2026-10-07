/**
 * How a server address is named to a person: the host, the port unless it is
 * the scheme's default, and the path when there is one, without the scheme.
 * Text with no host in it, whether it is not a URL at all or was typed
 * without a scheme (`localhost:8080` parses as the scheme `localhost:` with no
 * host), is repeated as it stands. A blank address has no name: the caller
 * decides what a blank means where it is.
 */
export function serverName(address: string): string {
  const trimmed = address.trim();
  try {
    const url = new URL(trimmed);
    if (url.hostname === "") return trimmed;
    const path = url.pathname.replace(/\/+$/, "");
    return `${url.host}${path}`;
  } catch {
    return trimmed;
  }
}
