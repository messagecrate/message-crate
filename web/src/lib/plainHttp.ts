/**
 * True when `address` is a plain `http:` address on another computer, so a
 * password typed for it crosses the network unencrypted. A loopback host
 * (`localhost`, any `127.x.x.x`, `[::1]`) never leaves this computer, which
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

/** `URL.hostname` brackets an IPv6 address and writes an IPv4 one in full. */
function isLoopbackHost(hostname: string): boolean {
  return hostname === "localhost" || hostname === "[::1]" || /^127\.\d+\.\d+\.\d+$/.test(hostname);
}
