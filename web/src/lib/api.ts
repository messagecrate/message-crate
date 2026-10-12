import { appHeaders } from "./build";
import { createExternalStore } from "./externalStore";
import { startTimeLimit } from "./timeLimit";

let baseUrl = "";
let authToken: string | null = null;
const accountId = createExternalStore<number | null>(null);

/** Set the server URL. An empty string means "same host as this page". */
export function setBaseUrl(url: string) {
  baseUrl = url.replace(/\/+$/, "");
}

/** Store the session token used on later API calls. Pass null to log out. */
export function setToken(token: string | null) {
  authToken = token;
}

/** Current session token on the API client, or null when logged out. */
export function getToken(): string | null {
  return authToken;
}

/**
 * Remember which account the session token names. The server addresses an
 * account by id under `/v1/accounts/{id}`, and the credential's own id is
 * what `POST /v1/session` answered, so it is kept beside the token it
 * belongs to. Pass null to log out.
 */
export function setAccountId(id: number | null) {
  accountId.set(id);
}

/**
 * Call `listener` whenever the logged-in account changes, logging out
 * included. Returns the function that stops it.
 */
export const onAccountIdChange = accountId.subscribe;

/** The logged-in account's id, or null when logged out. */
export const getAccountId = accountId.get;

export function getBaseUrl(): string {
  return baseUrl;
}

/**
 * The body of every failure the server answers: an RFC 7807 problem document
 * (`docs/architecture/http-api.md`). `type` is the URL of the page describing the kind of failure,
 * or `about:blank` for an internal error.
 */
type Problem = {
  type: string;
  title: string;
  status: number;
  detail?: string;
  errors?: string[];
  request_id?: string;
  word?: string;
  did_you_mean?: string;
  retry_after?: number;
  line?: number;
};

/**
 * A raw-text fallback longer than this is someone else's page, not a message
 * — clamped so it cannot stretch the auth card, which grows to fit its
 * content and never scrolls.
 */
const RAW_BODY_FALLBACK_LIMIT = 200;

/**
 * A failed response from the server: the HTTP status, the problem's slug so a
 * screen can branch on what went wrong, and `message`, the sentence a person
 * reads — the problem's `detail`, or its `errors` joined for a validation
 * failure.
 */
export class ApiError extends Error {
  readonly status: number;
  /** The last segment of the problem's `type` URL; null when the body was not a problem. */
  readonly type: string | null;
  readonly title: string | null;
  readonly detail: string | null;
  readonly errors: string[];
  readonly requestId: string | null;
  /** True when the call's time limit ended it and no answer arrived (`requestTimeoutError`). */
  readonly timedOut: boolean;

  constructor(
    status: number,
    message: string,
    problem: Problem | null = null,
    { timedOut = false }: { timedOut?: boolean } = {},
  ) {
    super(message);
    this.name = "ApiError";
    this.status = status;
    this.timedOut = timedOut;
    this.type = problem ? problemSlug(problem.type) : null;
    this.title = problem?.title ?? null;
    this.detail = problem?.detail ?? null;
    this.errors = problem?.errors ?? [];
    this.requestId = problem?.request_id ?? null;
  }
}

function problemSlug(type: string): string | null {
  if (type === "about:blank") return null;
  const slug = type.slice(type.lastIndexOf("/") + 1);
  return slug ? slug : null;
}

function parseProblem(text: string): Problem | null {
  try {
    const parsed: unknown = JSON.parse(text);
    if (
      parsed &&
      typeof parsed === "object" &&
      "type" in parsed &&
      "title" in parsed &&
      typeof (parsed as { type: unknown }).type === "string" &&
      typeof (parsed as { title: unknown }).title === "string"
    ) {
      return parsed as Problem;
    }
  } catch {
    // Not JSON — the raw text is the best available message.
  }
  return null;
}

/**
 * The error to throw for a failed response.
 *
 * The server answers a problem document, and its `detail` (or, for a
 * validation failure, every one of its `errors`) is what a user should read —
 * not the status code and not the envelope around it. Anything else (a
 * proxy's HTML error page, an empty body) falls back to the raw text — clamped
 * to `RAW_BODY_FALLBACK_LIMIT` characters, since a reverse proxy or other
 * host can answer with a whole HTML page — then to a generic sentence.
 */
export function problemFromBody(status: number, text: string): ApiError {
  const trimmed = text.trim();
  if (!trimmed) return new ApiError(status, `Request failed (${status})`);

  const problem = parseProblem(trimmed);
  if (problem) {
    const detail = problem.detail?.trim();
    const errors = (problem.errors ?? []).map((e) => e.trim()).filter(Boolean);
    const message =
      detail || errors.join("; ") || problem.title.trim() || `Request failed (${status})`;
    return new ApiError(status, message, problem);
  }
  if (trimmed.length > RAW_BODY_FALLBACK_LIMIT) {
    return new ApiError(status, `${trimmed.slice(0, RAW_BODY_FALLBACK_LIMIT)}…`);
  }
  return new ApiError(status, trimmed);
}

/**
 * How every call that carries the Session's header treats a redirect: as a
 * network error. `/v1` never redirects (`docs/architecture/http-api.md`,
 * "Credentials and reach"), so a redirect comes from something in front of
 * the server, such as a misconfigured proxy, and following it could send
 * the Session token to another host in a WebView that keeps `Authorization`
 * across origins. The failure reads as an unreachable server.
 */
export const NEVER_FOLLOW: RequestRedirect = "error";

/**
 * How long a call waits for the server's whole answer, the body included,
 * unless its caller asks for longer. A server that takes the connection and
 * then never answers (a stalled container, a request stuck behind the
 * database's write lock) would otherwise leave a screen loading until the
 * browser's own limit, which can be minutes.
 */
export const REQUEST_TIMEOUT_MS = 30_000;

export type ApiRequestOptions = {
  signal?: AbortSignal;
  /** How long to wait for the answer; `REQUEST_TIMEOUT_MS` when not given. */
  timeoutMs?: number;
};

/** A time limit in words: "30 seconds", "10 minutes". */
function durationText(ms: number): string {
  if (ms >= 60_000 && ms % 60_000 === 0) {
    const minutes = ms / 60_000;
    return minutes === 1 ? "1 minute" : `${minutes} minutes`;
  }
  const seconds = Math.max(1, Math.round(ms / 1000));
  return seconds === 1 ? "1 second" : `${seconds} seconds`;
}

/**
 * The error for a call its time limit ended: an `ApiError` of status 0 (no
 * answer arrived, so there is no HTTP status) whose message says how long it
 * waited, so a screen shows a sentence rather than the browser's abort text.
 */
export function requestTimeoutError(ms: number): ApiError {
  return new ApiError(0, `The server didn't answer within ${durationText(ms)}.`, null, {
    timedOut: true,
  });
}

/**
 * The headers every call carries: this app and its Build, the Session's
 * token when there is one, and `contentType` when the call has a body.
 */
function callHeaders(contentType: string | null): Record<string, string> {
  const headers: Record<string, string> = { ...appHeaders() };
  if (contentType) {
    headers["Content-Type"] = contentType;
  }
  if (authToken) {
    headers.Authorization = `Bearer ${authToken}`;
  }
  return headers;
}

/**
 * Send one request and read its answer with `read`, within the call's time
 * limit. The limit covers the body as well as the headers, since a server
 * can stall partway through either, unless `untilHeaders` says it ends once
 * the answer starts: a download as large as a video then takes as long as it
 * needs, while a server that never answers still ends the call.
 *
 * A call the limit ends rejects with `requestTimeoutError`. A call its
 * caller's signal ends rejects with that abort, as before, since a cancel is
 * not a failure to show.
 */
async function exchange<T>(
  method: string,
  path: string,
  body: { text: string; contentType: string } | null,
  opts: ApiRequestOptions | undefined,
  read: (res: Response) => Promise<T>,
  { untilHeaders = false }: { untilHeaders?: boolean } = {},
): Promise<T> {
  const timeoutMs = opts?.timeoutMs ?? REQUEST_TIMEOUT_MS;
  const limit = startTimeLimit(timeoutMs, opts?.signal);
  try {
    const res = await fetch(`${baseUrl}${path}`, {
      method,
      headers: callHeaders(body?.contentType ?? null),
      body: body?.text,
      signal: limit.signal,
      redirect: NEVER_FOLLOW,
    });
    if (untilHeaders) limit.stopClock();
    return await read(res);
  } catch (e: unknown) {
    if (limit.timedOut()) throw requestTimeoutError(timeoutMs);
    throw e;
  } finally {
    limit.dispose();
  }
}

/**
 * A JSON body, or none. A request says it carries JSON only when it does: the
 * server reads a body wherever the media type promises one, so an empty
 * DELETE marked as JSON is refused as unparseable.
 */
function jsonBody(body: unknown): { text: string; contentType: string } | null {
  return body ? { text: JSON.stringify(body), contentType: "application/json" } : null;
}

/** Read a JSON answer: a failure's problem document, nothing for a 204, or the body. */
async function readJson<T>(res: Response): Promise<T> {
  if (!res.ok) {
    const text = await res.text();
    throw problemFromBody(res.status, text);
  }

  // A 204 has no body by definition; asking for JSON would throw.
  if (res.status === 204) {
    return undefined as T;
  }

  return (await res.json()) as T;
}

/** Read a file's text, or a failure's problem document as everywhere else. */
async function readText(res: Response): Promise<string> {
  const text = await res.text();
  if (!res.ok) {
    throw problemFromBody(res.status, text);
  }
  return text;
}

/** Read an answer's bytes, or a failure's problem document as everywhere else. */
async function readBlob(res: Response): Promise<Blob> {
  if (!res.ok) {
    const text = await res.text();
    throw problemFromBody(res.status, text);
  }
  return res.blob();
}

function request<T>(
  method: string,
  path: string,
  body: unknown,
  opts?: ApiRequestOptions,
): Promise<T> {
  return exchange(method, path, jsonBody(body), opts, readJson<T>);
}

export const apiClient = {
  get<T>(path: string, opts?: ApiRequestOptions): Promise<T> {
    return request<T>("GET", path, undefined, opts);
  },
  /** POST a body that is not JSON — a file's text — under its own media type. */
  postRaw<T>(
    path: string,
    body: string,
    contentType: string,
    opts?: ApiRequestOptions,
  ): Promise<T> {
    return exchange("POST", path, { text: body, contentType }, opts, readJson<T>);
  },
  /** POST a JSON body and read the answer as text: a file the server writes. */
  postText(path: string, body: unknown, opts?: ApiRequestOptions): Promise<string> {
    return exchange("POST", path, jsonBody(body), opts, readText);
  },
  /** GET a route that answers a file's text instead of JSON. */
  getText(path: string, opts?: ApiRequestOptions): Promise<string> {
    return exchange("GET", path, null, opts, readText);
  },
  /**
   * GET a route that answers bytes, such as an attachment, as a Blob. The time
   * limit covers only the wait for the answer to start, so a large file that
   * is arriving is never cut off.
   */
  getBlob(path: string, opts?: ApiRequestOptions): Promise<Blob> {
    return exchange("GET", path, null, opts, readBlob, { untilHeaders: true });
  },
  post<T>(path: string, body?: unknown, opts?: ApiRequestOptions): Promise<T> {
    return request<T>("POST", path, body, opts);
  },
  put<T>(path: string, body?: unknown, opts?: ApiRequestOptions): Promise<T> {
    return request<T>("PUT", path, body, opts);
  },
  patch<T>(path: string, body?: unknown, opts?: ApiRequestOptions): Promise<T> {
    return request<T>("PATCH", path, body, opts);
  },
  delete<T>(path: string, body?: unknown, opts?: ApiRequestOptions): Promise<T> {
    return request<T>("DELETE", path, body, opts);
  },
};
