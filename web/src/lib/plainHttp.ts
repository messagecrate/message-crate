/**
 * True when `address` is a plain `http:` address on another computer, so a
 * password typed for it crosses the network unencrypted. A loopback host
 * (`isLoopbackHost`) never leaves this computer, which
 * also covers the desktop app's own Message Crate at `127.0.0.1:8080`. An
 * `https:` address is encrypted, and text that is not an address names no
 * network at all, so neither is one.
 */
export function isPlainHttpOverNetwork(address: string): boolean {
  let url: URL;
  try {
    url = new URL(address.trim());
  } catch {
    return false;
  }
  if (url.protocol !== "http:" || url.hostname === "") return false;
  return !isLoopbackHost(url.hostname);
}

/**
 * Hosts that name this computer. `URL.hostname` brackets an IPv6 address,
 * writes an IPv4 one in full, and writes an IPv4-mapped loopback such as
 * `[::ffff:127.0.0.1]` as `[::ffff:7f00:1]`. Browsers send any `*.localhost`
 * name and `0.0.0.0` to this computer too.
 */
function isLoopbackHost(hostname: string): boolean {
  return (
    hostname === "localhost" ||
    hostname.endsWith(".localhost") ||
    hostname === "0.0.0.0" ||
    hostname === "[::1]" ||
    /^127\.\d+\.\d+\.\d+$/.test(hostname) ||
    /^\[::ffff:7f[0-9a-f]{2}:[0-9a-f]{1,4}\]$/.test(hostname)
  );
}
