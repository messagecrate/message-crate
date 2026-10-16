/** The query parameter of `/owner/logs` that names a request whose lines to show. */
export const REQUEST_PARAM = "request";

/**
 * The address of Logs opened on the server's log at one request's lines:
 * where an Audit Trail entry's "View in log" goes (#3060).
 */
export function serverLogOfRequest(requestId: string): string {
  return `/owner/logs?${new URLSearchParams({ [REQUEST_PARAM]: requestId })}`;
}
