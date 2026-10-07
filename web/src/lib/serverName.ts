/** The port a scheme implies when the address leaves it out. */
const DEFAULT_PORT: Record<string, string> = { "http:": "80", "https:": "443" };

/**
 * How a server address is named to a person: the host and port, without the
 * scheme. The port is always written, because once the scheme is gone nothing
 * else says which default it stood for. A blank address is the website's own
 * origin, as it is for the API client. An address that is not a URL, such as
 * one still being typed, is repeated as it stands, since there is no host to
 * pull out of it.
 */
export function serverName(address: string): string {
  const trimmed = address.trim();
  const absolute = trimmed === "" ? window.location.origin : trimmed;
  try {
    const url = new URL(absolute);
    const port = url.port || DEFAULT_PORT[url.protocol] || "";
    return port ? `${url.hostname}:${port}` : url.hostname;
  } catch {
    return trimmed;
  }
}
