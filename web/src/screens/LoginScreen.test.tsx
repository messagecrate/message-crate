/** @vitest-environment jsdom */

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const login = vi.fn();
const setServer = vi.fn();
const retrySavedLogin = vi.fn();

const authState = vi.hoisted(() => ({ serverUrl: "" }));

vi.mock("../lib/authContext", () =>
  authModule(() => ({ login, setServer, retrySavedLogin, serverUrl: authState.serverUrl })),
);

const tauriState = vi.hoisted(() => ({ isTauri: false }));
const startLocalServer = vi.hoisted(() => vi.fn());
const invokeLocalServerStatus = vi.hoisted(() => vi.fn());
const invokeOpenDataDirectory = vi.hoisted(() => vi.fn());

vi.mock("../lib/tauriCheck", () => ({
  isTauri: () => tauriState.isTauri,
}));

vi.mock("../lib/localServer", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/localServer")>()),
  startLocalServer: () => startLocalServer(),
}));

vi.mock("../lib/tauri", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/tauri")>()),
  invokeLocalServerStatus: () => invokeLocalServerStatus(),
  invokeOpenDataDirectory: () => invokeOpenDataDirectory(),
}));

const setBaseUrlSpy = vi.hoisted(() => vi.fn());

vi.mock("../lib/api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../lib/api")>();
  return {
    ...actual,
    setBaseUrl: (url: string) => {
      setBaseUrlSpy(url);
      actual.setBaseUrl(url);
    },
  };
});

// The card asks an address two things: whether it is healthy, and what state
// its Message Crate is in. Logging in to the Demo Account is a third. Each is
// faked by its name, as ADR 0002 says, so a renamed route is the drift test's
// business in `serverApiOpenapi.test.ts` and not something this file passes over.
vi.mock("../lib/serverApi", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/serverApi")>()),
  getServerState: vi.fn(),
  login: vi.fn(),
}));

vi.mock("../lib/serverHealth", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/serverHealth")>()),
  checkServerHealth: vi.fn(),
  checkServerHealthWithRetry: vi.fn(),
}));

import { getBaseUrl } from "../lib/api";
import { getServerState, login as serverLogin } from "../lib/serverApi";
import {
  checkServerHealth,
  checkServerHealthWithRetry,
  HEALTH_RECHECK_AFTER_FAILURE_MS,
  HEALTH_URL_DEBOUNCE_MS,
} from "../lib/serverHealth";
import { STARTING_POLL_MS } from "../lib/useLocalServer";
import type { ServerState } from "../lib/useServerState";
import { advanceTimersAndSettle, settle, startFakeClock, stopFakeClock } from "../test/fakeTimers";
import { authModule, Providers } from "../test/providers";
import { fill, setupUser } from "../test/user";
import LoginScreen from "./LoginScreen";

const checkServerHealthMock = vi.mocked(checkServerHealth);
const checkServerHealthWithRetryMock = vi.mocked(checkServerHealthWithRetry);
const getServerStateMock = vi.mocked(getServerState);
const serverLoginMock = vi.mocked(serverLogin);

/**
 * The card's connect asks again after a server's error before it says
 * Disconnected. Most tests are about what the card shows once it has its
 * answer, so by default connect makes one ask of the faked probe; the tests
 * of the retry put the real one back over a faked `fetch`.
 */
function skipRetry() {
  checkServerHealthWithRetryMock.mockImplementation((address, signal) =>
    checkServerHealthMock(address, signal),
  );
}

async function retryForReal() {
  const actual = await vi.importActual<typeof import("../lib/serverHealth")>("../lib/serverHealth");
  checkServerHealthWithRetryMock.mockImplementation(actual.checkServerHealthWithRetry);
}

/**
 * What a Message Crate at one address does: answers with its state, refuses
 * the connection, or never answers at all.
 */
type Answer = { state: ServerState; demoAccount?: boolean } | "down" | "silent";

/**
 * Fake every address by what `answer` says it does. The health probe names
 * the address it asks; the server's state is asked of the address
 * `useServerState` has just made the base URL, which `getBaseUrl` reads back.
 */
function serveAt(answer: (address: string) => Answer) {
  checkServerHealthMock.mockImplementation(async (address) => {
    const answered = answer(address);
    if (answered === "silent") return new Promise<boolean>(() => {});
    return answered !== "down";
  });
  getServerStateMock.mockImplementation(async () => {
    const answered = answer(getBaseUrl());
    if (answered === "silent") return new Promise<never>(() => {});
    if (answered === "down") throw new TypeError("Failed to fetch");
    return {
      id: "0123456789abcdef0123456789abcdef",
      state: answered.state,
      demo_account: answered.demoAccount ?? false,
      version: "0.10.0",
      schema_fingerprint: 1,
      asset_max_bytes: 1024,
      upload_stall_secs: 60,
      upload_piece_secs: 300,
    };
  });
}

/**
 * Every address healthy, in the state given.
 *
 * The state decides which forms the card offers, so a test that says nothing
 * about it gets `open`: the two-tab card, which is what most of these tests
 * are about.
 */
function stubServer(state: ServerState = "open", demoAccount = false) {
  serveAt(() => ({ state, demoAccount }));
}

/** A server nothing answers at, as the app's own is while it starts. */
function stubNoServer() {
  serveAt(() => "down");
}

function renderScreen() {
  render(
    <Providers>
      <MemoryRouter>
        <LoginScreen />
      </MemoryRouter>
    </Providers>,
  );
}

// `setupUser` fires every keystroke without a timer between them. With
// user-event's default delay, an address typed a character at a time on a
// loaded machine can take longer than the card's own 400ms health re-probe
// debounce. That is long enough for the background probe to race the explicit
// reconnect this screen triggers.

describe("LoginScreen", () => {
  beforeEach(() => {
    login.mockReset();
    setServer.mockReset();
    tauriState.isTauri = false;
    authState.serverUrl = "";
    startLocalServer.mockReset();
    startLocalServer.mockResolvedValue({ status: "ready", started_by_app: true });
    invokeLocalServerStatus.mockReset();
    invokeLocalServerStatus.mockResolvedValue({ status: "ready", started_by_app: true });
    invokeOpenDataDirectory.mockReset();
    invokeOpenDataDirectory.mockResolvedValue(undefined);
    setBaseUrlSpy.mockReset();
    retrySavedLogin.mockReset();
    checkServerHealthMock.mockReset();
    checkServerHealthWithRetryMock.mockReset();
    skipRetry();
    getServerStateMock.mockReset();
    serverLoginMock.mockReset();
    startFakeClock();
  });

  afterEach(() => {
    cleanup();
    stopFakeClock();
  });

  it("logs in without a server-selection step", async () => {
    stubServer();
    renderScreen();
    await settle();

    expect(screen.getByRole("tab", { name: "Login" })).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "Username" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Log in" })).toBeInTheDocument();

    expect(screen.queryByRole("button", { name: "Connect" })).not.toBeInTheDocument();
    expect(screen.queryByRole("textbox", { name: "Server URL" })).not.toBeInTheDocument();
  });

  it("offers the Demo Account beside Create Owner on an unclaimed Message Crate", async () => {
    stubServer("unclaimed", true);
    serverLoginMock.mockResolvedValue({ token: "mc-user-demo", account_id: 2, username: "demo" });
    renderScreen();
    await settle();

    const explore = screen.getByRole("button", { name: "Explore Demo Account" });
    expect(screen.getByRole("heading", { name: "Create Owner" })).toBeInTheDocument();

    await setupUser().click(explore);
    await settle();

    expect(login).toHaveBeenCalledWith("", "mc-user-demo", 2);
    expect(serverLoginMock).toHaveBeenCalledWith({ username: "demo", password: "" });
  });

  it("keeps the Demo Account button beside the login form once claimed", async () => {
    stubServer("closed", true);
    renderScreen();
    await settle();

    expect(screen.getByRole("button", { name: "Explore Demo Account" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Log in" })).toBeInTheDocument();
  });

  it("shows no Demo Account button when the server has no Demo Account", async () => {
    stubServer("closed", false);
    renderScreen();
    await settle();

    expect(screen.getByRole("button", { name: "Log in" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Explore Demo Account" })).not.toBeInTheDocument();
  });

  it("names the product and the server the card is connected to", async () => {
    stubServer();
    renderScreen();
    await settle();

    // A blank address is the website's own origin, named by host and port.
    const origin = new URL(window.location.origin);
    expect(screen.getByRole("status")).toHaveTextContent(
      new RegExp(`^Connected to ${origin.hostname}:${origin.port || "80"}$`),
    );
    expect(screen.getByRole("heading", { name: "Message Crate" })).toBeInTheDocument();
    expect(setServer).toHaveBeenCalledWith("");
  });

  it("names a server on another computer by host and port, without the scheme", async () => {
    authState.serverUrl = "http://192.168.1.20:9000";
    stubServer();
    renderScreen();
    await settle();

    expect(screen.getByText("Connected to 192.168.1.20:9000")).toBeInTheDocument();
    expect(screen.queryByText(/http:\/\/192\.168\.1\.20/)).not.toBeInTheDocument();
  });

  // The forms send the password to the address the card is on, so over
  // plain HTTP to another computer the card says it crosses the network
  // unencrypted, on the login form and on Create Owner alike (#2115).
  const IN_CLEAR = /not encrypted, so the password crosses the network as typed/;

  it("says a plain HTTP connection to another computer is not encrypted", async () => {
    authState.serverUrl = "http://192.168.1.20:8080";
    stubServer("closed");
    renderScreen();
    await settle();

    expect(screen.getByRole("button", { name: "Log in" })).toBeInTheDocument();
    expect(screen.getByText(IN_CLEAR)).toBeInTheDocument();
  });

  it("says so above Create Owner too", async () => {
    authState.serverUrl = "http://192.168.1.20:8080";
    stubServer("unclaimed");
    renderScreen();
    await settle();

    expect(screen.getByRole("heading", { name: "Create Owner" })).toBeInTheDocument();
    expect(screen.getByText(IN_CLEAR)).toBeInTheDocument();
  });

  it("says nothing of encryption for this computer or an https address", async () => {
    for (const address of ["", "http://127.0.0.1:8080", "https://192.168.1.20:8080"]) {
      authState.serverUrl = address;
      stubServer("closed");
      renderScreen();
      await settle();

      expect(screen.getByRole("button", { name: "Log in" })).toBeInTheDocument();
      expect(screen.queryByText(IN_CLEAR), address).not.toBeInTheDocument();
      cleanup();
    }
  });

  it("keeps both tabs, Login first", async () => {
    stubServer();
    renderScreen();
    await settle();

    expect(screen.getByRole("tab", { name: "Login" })).toBeInTheDocument();
    const tabs = screen.getAllByRole("tab");
    expect(tabs.map((t) => t.textContent)).toEqual(["Login", "Create Account"]);
    expect(tabs[0]).toHaveAttribute("aria-selected", "true");
  });

  it("still asks for the password twice on Create Account", async () => {
    stubServer();
    const user = setupUser();
    renderScreen();
    await settle();

    expect(screen.getByRole("tab", { name: "Create Account" })).toBeInTheDocument();
    await user.click(screen.getByRole("tab", { name: "Create Account" }));

    expect(screen.getByLabelText("Password")).toBeInTheDocument();
    expect(screen.getByLabelText("Confirm Password")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Continue" })).toBeInTheDocument();
  });

  it("drops the password-length claim the server does not enforce", async () => {
    stubServer();
    const user = setupUser();
    renderScreen();
    await settle();

    expect(screen.getByRole("tab", { name: "Create Account" })).toBeInTheDocument();
    await user.click(screen.getByRole("tab", { name: "Create Account" }));

    expect(screen.queryByText(/At least 8 characters/i)).not.toBeInTheDocument();
  });

  it("rejects a new account when the two passwords disagree", async () => {
    stubServer();
    const user = setupUser();
    renderScreen();
    await settle();

    expect(screen.getByRole("tab", { name: "Create Account" })).toBeInTheDocument();
    await user.click(screen.getByRole("tab", { name: "Create Account" }));
    await fill(user, screen.getByRole("textbox", { name: "Username" }), "ada");
    await fill(user, screen.getByLabelText("Password"), "hunter22");
    await fill(user, screen.getByLabelText("Confirm Password"), "hunter23");
    await user.click(screen.getByRole("button", { name: "Continue" }));

    expect(screen.getByText("Passwords do not match.")).toBeInTheDocument();
    expect(login).not.toHaveBeenCalled();
  });

  it("says Disconnected when nothing answers", async () => {
    stubNoServer();
    renderScreen();
    await settle();

    expect(screen.getByText(/^Disconnected from /)).toBeInTheDocument();
    // The address field belongs to the settings screen now, not the card.
    expect(screen.queryByRole("textbox", { name: "Address" })).not.toBeInTheDocument();
  });

  it("asks once more before it says Disconnected, so one missed probe does not show it", async () => {
    // A server whose database an import keeps busy can fail one /health and
    // answer the next (#2718).
    stubServer();
    await retryForReal();
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce({ ok: false, status: 500 })
      .mockResolvedValue({ ok: true });
    vi.stubGlobal("fetch", fetchMock);
    renderScreen();
    await settle();

    await advanceTimersAndSettle(HEALTH_RECHECK_AFTER_FAILURE_MS - 1);
    expect(fetchMock).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("status")).toHaveTextContent(/^Connecting to /);

    await advanceTimersAndSettle(1);
    expect(fetchMock).toHaveBeenCalledTimes(2);
    expect(screen.getByRole("status")).toHaveTextContent(/^Connected to /);
  });

  it("says Disconnected once the second probe in a row fails too", async () => {
    stubNoServer();
    await retryForReal();
    const fetchMock = vi.fn().mockResolvedValue({ ok: false, status: 500 });
    vi.stubGlobal("fetch", fetchMock);
    renderScreen();
    await settle();

    await advanceTimersAndSettle(HEALTH_RECHECK_AFTER_FAILURE_MS);
    expect(fetchMock).toHaveBeenCalledTimes(2);
    expect(screen.getByRole("status")).toHaveTextContent(/^Disconnected from /);
  });

  it("shows the login form, disabled, when nothing answers", async () => {
    // A skeleton reads as "still loading". A card that has its answer — no
    // server — has to look finished, or the screen seems to hang.
    stubNoServer();
    renderScreen();
    await settle();

    expect(screen.getByText(/^Disconnected from /)).toBeInTheDocument();
    expect(screen.queryByTestId("auth-form-skeleton")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Log in" })).toBeDisabled();
    // No server has said it takes new accounts, so the card does not offer one.
    expect(screen.queryByRole("tab", { name: "Create Account" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Change server address" })).toBeEnabled();
  });

  it("shows the placeholder form only while it is still connecting", async () => {
    serveAt(() => "silent");
    renderScreen();
    await settle();

    expect(screen.getByText(/^Connecting to /)).toBeInTheDocument();
    expect(screen.getByTestId("auth-form-skeleton")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Log in" })).not.toBeInTheDocument();
  });

  it("lets the server be changed while the card is still connecting", async () => {
    // A server that never answers holds the card in "connecting": a wrong
    // address is exactly when you need the settings screen most, so the way
    // to it must not wait for the probe to give up.
    serveAt(() => "silent");
    const user = setupUser();
    renderScreen();
    await settle();

    expect(screen.getByText(/^Connecting to /)).toBeInTheDocument();
    const link = screen.getByRole("button", { name: "Change server address" });
    expect(link).toBeEnabled();

    await user.click(link);
    expect(screen.getByRole("heading", { name: "Server Address" })).toBeInTheDocument();
  });

  it("keeps the way out of a red card live", async () => {
    stubNoServer();
    renderScreen();
    await settle();

    expect(screen.getByText(/^Disconnected from /)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Change server address" })).toBeEnabled();
  });

  it("stays on the Message Crate it has and names the address that did not answer", async () => {
    // A answers and B does not. The card is connected to A, B is applied, and
    // the card must neither move to B nor slip back to A without a word.
    const A = "http://crate-a.example:8080";
    const B = "http://crate-b.example:8080";
    authState.serverUrl = A;
    serveAt((address) => (address.startsWith(A) ? { state: "closed" } : "down"));
    const user = setupUser();
    renderScreen();
    await settle();

    expect(screen.getByText(/^Connected to /)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Change server address" }));
    const field = screen.getByRole("textbox", { name: "Address" });
    await user.clear(field);
    await user.type(field, B);
    await user.click(screen.getByRole("button", { name: "Use this address" }));
    await settle();

    expect(screen.getByRole("alert")).toHaveTextContent(
      `Nothing answered at ${B}. Still connected to ${A}.`,
    );
    // The probe has run by now, had the card gone back to watching A as a
    // disconnected card.
    await advanceTimersAndSettle(HEALTH_URL_DEBOUNCE_MS);
    expect(screen.getByRole("status")).toHaveTextContent("Connected to crate-a.example:8080");
    expect(screen.getByRole("button", { name: "Log in" })).toBeEnabled();
    expect(setServer).not.toHaveBeenCalledWith(B);
    expect(setBaseUrlSpy).not.toHaveBeenCalledWith(B);
    expect(setBaseUrlSpy).toHaveBeenCalledWith(A);
    expect(retrySavedLogin).not.toHaveBeenCalled();
  });

  it("says a disconnected card is still disconnected when the new address does not answer either", async () => {
    stubNoServer();
    const user = setupUser();
    renderScreen();
    await settle();

    expect(screen.getByText(/^Disconnected from /)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Change server address" }));
    const field = screen.getByRole("textbox", { name: "Address" });
    await user.clear(field);
    await fill(user, field, "http://127.0.0.1:9999");
    await user.click(screen.getByRole("button", { name: "Use this address" }));
    await settle();

    expect(screen.getByRole("alert")).toHaveTextContent(
      "Nothing answered at http://127.0.0.1:9999.",
    );
    expect(screen.getByRole("status")).toHaveTextContent("Disconnected");
    expect(screen.getByRole("button", { name: "Log in" })).toBeDisabled();
    expect(setBaseUrlSpy).not.toHaveBeenCalledWith("http://127.0.0.1:9999");
  });

  it("offers Use this address only for an address that is a change", async () => {
    stubServer();
    const user = setupUser();
    renderScreen();
    await settle();

    expect(screen.getByText(/^Connected to /)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Change server address" }));

    const field = screen.getByRole("textbox", { name: "Address" });
    const apply = () => screen.getByRole("button", { name: "Use this address" });

    // The card connected to the address the field already holds, so there is
    // nothing to apply.
    expect(apply()).toBeDisabled();

    await fill(user, field, "http://127.0.0.1:8080");
    expect(apply()).toBeEnabled();

    // A field naming no address at all is nothing to apply either.
    await user.clear(field);
    expect(apply()).toBeDisabled();

    // Cancel is unaffected and stays the way out.
    expect(screen.getByRole("button", { name: "Cancel" })).toBeEnabled();

    await fill(user, field, "http://127.0.0.1:8080");
    await user.click(apply());
    await settle();
    expect(setServer).toHaveBeenCalledWith("http://127.0.0.1:8080");

    // Back on the settings screen, the applied address is now the connected
    // one, so it is no longer a change — and editing it makes it one again.
    await user.click(screen.getByRole("button", { name: "Change server address" }));
    expect(screen.getByRole("textbox", { name: "Address" })).toHaveValue("http://127.0.0.1:8080");
    expect(apply()).toBeDisabled();

    await user.type(screen.getByRole("textbox", { name: "Address" }), "9");
    expect(apply()).toBeEnabled();
  });

  it("opens Server Address from the link and comes back on Cancel", async () => {
    stubServer();
    const user = setupUser();
    renderScreen();
    await settle();

    expect(screen.getByRole("tab", { name: "Login" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Change server address" }));

    expect(screen.getByRole("heading", { name: "Server Address" })).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "Address" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Test" })).toBeInTheDocument();
    // The settings screen replaces the card body rather than opening beside it.
    expect(screen.queryByRole("tab", { name: "Login" })).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(screen.getByRole("tab", { name: "Login" })).toBeInTheDocument();
  });

  it("reports what Test found for the typed address", async () => {
    stubServer();
    const user = setupUser();
    renderScreen();
    await settle();

    expect(screen.getByRole("tab", { name: "Login" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Change server address" }));

    stubNoServer();
    const field = screen.getByRole("textbox", { name: "Address" });
    await user.clear(field);
    await fill(user, field, "http://127.0.0.1:9999");
    await user.click(screen.getByRole("button", { name: "Test" }));
    await settle();

    expect(screen.getByText(/^Disconnected from /)).toBeInTheDocument();
    // Testing does not commit the address: the card is still connected behind.
    expect(setServer).not.toHaveBeenCalledWith("http://127.0.0.1:9999");
  });

  it("does not credit an edited address with the connection it never earned", async () => {
    stubServer();
    const user = setupUser();
    renderScreen();
    await settle();

    expect(screen.getByRole("tab", { name: "Login" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Change server address" }));
    // Opened on the address the card is connected to, so that connection is
    // this address's and saying so is true.
    expect(screen.getByRole("status")).toHaveTextContent("Connected");

    stubNoServer();
    const field = screen.getByRole("textbox", { name: "Address" });
    await fill(user, field, "http://127.0.0.1:9999");
    // Typed but never tried: the card is still connected behind this screen,
    // but not to what is in the box.
    expect(screen.getByRole("status")).toHaveTextContent("Not tested: 127.0.0.1:9999");

    await user.click(screen.getByRole("button", { name: "Test" }));
    await settle();
    expect(screen.getByText("Disconnected from 127.0.0.1:9999")).toBeInTheDocument();

    // Editing after a failed test clears that answer without inventing a
    // better one. A green here would say the typed address works.
    await user.type(field, "9");
    expect(screen.getByRole("status")).toHaveTextContent("Not tested");
  });

  it("applies a typed address and reconnects", async () => {
    stubNoServer();
    const user = setupUser();
    renderScreen();
    await settle();

    expect(screen.getByText(/^Disconnected from /)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Change server address" }));

    // Only the address being typed answers healthy — the disconnected card's
    // own background self-heal probe (`useServerHealth`) keeps polling the
    // blank address it was last on, a different host from the one typed
    // below. Two hosts, two answers, so this test is about the one thing it
    // names: applying the address that was typed. Which probe wins when both
    // answer is the next test's subject.
    // Healthy, and open, so the card offers the two tabs this test looks for.
    serveAt((address) =>
      address.startsWith("http://127.0.0.1:8080") ? { state: "open" } : "down",
    );
    const field = screen.getByRole("textbox", { name: "Address" });
    await user.clear(field);
    await fill(user, field, "http://127.0.0.1:8080");
    await user.click(screen.getByRole("button", { name: "Use this address" }));
    await settle();

    expect(screen.getByRole("tab", { name: "Login" })).toBeInTheDocument();
    expect(setServer).toHaveBeenCalledWith("http://127.0.0.1:8080");
  });

  it("keeps the typed address when a slow probe for the old one answers late", async () => {
    // The card probes its saved address on mount. That probe is held open
    // here, so it is still in flight while a different address is typed and
    // submitted below — the two `connect()` calls the screen can have running
    // at once. The saved address is blank (`useAuth` above); the typed one is
    // absolute, so the fake can tell them apart and answer them in the order
    // this test needs.
    let answerSavedAddress: (() => void) | undefined;
    serveAt((address) =>
      address.startsWith("http://127.0.0.1:8080") ? { state: "open" } : "silent",
    );
    checkServerHealthMock.mockImplementation((address) =>
      address.startsWith("http://127.0.0.1:8080")
        ? Promise.resolve(true)
        : new Promise((resolve) => {
            answerSavedAddress = () => resolve(true);
          }),
    );
    const user = setupUser();
    renderScreen();
    await settle();

    expect(screen.getByText(/^Connecting to /)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Change server address" }));
    const field = screen.getByRole("textbox", { name: "Address" });
    await user.clear(field);
    await fill(user, field, "http://127.0.0.1:8080");
    await user.click(screen.getByRole("button", { name: "Use this address" }));
    await settle();

    expect(setServer).toHaveBeenCalledWith("http://127.0.0.1:8080");

    // Now the saved address answers healthy, after the typed one has already
    // been applied. It describes a server this screen has moved on from, and a
    // probe nobody is waiting on any more may not speak for the card. The
    // settle lets everything that answer starts land before the test reads.
    answerSavedAddress?.();
    await settle();

    expect(setServer).not.toHaveBeenCalledWith("");
    expect(setServer).toHaveBeenLastCalledWith("http://127.0.0.1:8080");

    // Reopening reads the address back out of the card, which is what the
    // late probe would have rewritten.
    await user.click(screen.getByRole("button", { name: "Change server address" }));
    expect(screen.getByRole("textbox", { name: "Address" })).toHaveValue("http://127.0.0.1:8080");
  });

  it("reconnects on its own once a probe finds the server healthy again", async () => {
    let healthy = false;
    serveAt(() => (healthy ? { state: "open" } : "down"));
    renderScreen();
    await settle();

    expect(screen.getByText(/^Disconnected from /)).toBeInTheDocument();

    healthy = true;

    // The disconnected card's probe asks only once its debounce has passed.
    await advanceTimersAndSettle(HEALTH_URL_DEBOUNCE_MS - 1);
    expect(screen.getByText(/^Disconnected from /)).toBeInTheDocument();
    await advanceTimersAndSettle(1);
    expect(screen.getByRole("tab", { name: "Login" })).toBeInTheDocument();
    expect(screen.getByText(/^Connected to /)).toBeInTheDocument();
  });

  it("tries the saved login again once the server is healthy again", async () => {
    let healthy = false;
    serveAt(() => (healthy ? { state: "open" } : "down"));
    retrySavedLogin.mockClear();
    renderScreen();
    await settle();

    expect(screen.getByText(/^Disconnected from /)).toBeInTheDocument();
    expect(retrySavedLogin).not.toHaveBeenCalled();

    healthy = true;

    await advanceTimersAndSettle(HEALTH_URL_DEBOUNCE_MS - 1);
    expect(retrySavedLogin).not.toHaveBeenCalled();
    await advanceTimersAndSettle(1);
    expect(retrySavedLogin).toHaveBeenCalledTimes(1);
    expect(retrySavedLogin).toHaveBeenCalledWith("");
  });

  it("probes the health of the address it is on, with an abort signal", async () => {
    stubServer();
    renderScreen();
    await settle();

    expect(screen.getByRole("tab", { name: "Login" })).toBeInTheDocument();
    expect(checkServerHealthMock).toHaveBeenCalledWith("", expect.any(AbortSignal));
  });

  it("puts the credentials in a real form, so a password manager can fill it", async () => {
    stubServer();
    renderScreen();
    await settle();

    const password = screen.getByLabelText("Password");
    // A password field outside a form is one browsers decline to offer to save.
    expect(password.closest("form")).not.toBeNull();
    expect(screen.getByRole("button", { name: "Log in" })).toHaveAttribute("type", "submit");
  });

  // The fields carry no Enter handler of their own any more: submitting is the
  // form's job, which is what lets the browser submit on Enter by itself.
  // jsdom does not perform that implicit submission, so this drives the form
  // element directly — that the key reaches it is the browser's part.
  it("runs the login from the form's own submit event", async () => {
    stubServer();
    renderScreen();
    await settle();

    expect(screen.getByRole("tab", { name: "Login" })).toBeInTheDocument();
    const form = screen.getByLabelText("Password").closest("form");
    expect(form).not.toBeNull();

    fireEvent.submit(form as HTMLFormElement);
    await settle();

    // Submitting with the username still empty reaches the handler's own check,
    // which is enough to show the form is what drives it.
    expect(screen.getByText("Username is required.")).toBeInTheDocument();
  });

  it("calls the new-account action Continue, since profile setup finishes it", async () => {
    stubServer();
    const user = setupUser();
    renderScreen();
    await settle();

    expect(screen.getByRole("tab", { name: "Create Account" })).toBeInTheDocument();
    await user.click(screen.getByRole("tab", { name: "Create Account" }));

    expect(screen.getByRole("button", { name: "Continue" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Create account" })).not.toBeInTheDocument();
  });

  it("keeps the action under the fields and the error down by the or-rule", async () => {
    stubServer();
    const user = setupUser();
    renderScreen();
    await settle();

    expect(screen.getByRole("tab", { name: "Create Account" })).toBeInTheDocument();
    await user.click(screen.getByRole("tab", { name: "Create Account" }));
    await fill(user, screen.getByRole("textbox", { name: "Username" }), "ada");
    await fill(user, screen.getByLabelText("Password"), "hunter22");
    await fill(user, screen.getByLabelText("Confirm Password"), "hunter23");
    await user.click(screen.getByRole("button", { name: "Continue" }));

    const message = screen.getByText("Passwords do not match.");
    const confirmField = screen.getByLabelText("Confirm Password");
    const action = screen.getByRole("button", { name: "Continue" });
    const orRule = screen.getByText("or");

    // Document order stands in for the layout: field, then action, then the
    // message, then the rule that closes the card.
    const precedes = (a: Element, b: Element) =>
      Boolean(a.compareDocumentPosition(b) & Node.DOCUMENT_POSITION_FOLLOWING);
    expect(precedes(confirmField, action)).toBe(true);
    expect(precedes(action, message)).toBe(true);
    expect(precedes(message, orRule)).toBe(true);
  });

  it("starts the desktop app's own Message Crate at the app's own address", async () => {
    tauriState.isTauri = true;
    stubServer();
    renderScreen();
    await settle();

    expect(screen.getByRole("tab", { name: "Login" })).toBeInTheDocument();
    expect(startLocalServer).toHaveBeenCalledTimes(1);
  });

  it("starts nothing for an address the person entered", async () => {
    tauriState.isTauri = true;
    stubServer();
    const user = setupUser();
    renderScreen();
    await settle();
    expect(screen.getByRole("tab", { name: "Login" })).toBeInTheDocument();
    startLocalServer.mockClear();

    await user.click(screen.getByRole("button", { name: "Change server address" }));
    const field = screen.getByRole("textbox");
    await user.clear(field);
    await fill(user, field, "http://crate.example:8080");
    await user.click(screen.getByRole("button", { name: "Use this address" }));
    await settle();

    expect(setServer).toHaveBeenCalledWith("http://crate.example:8080");
    expect(startLocalServer).not.toHaveBeenCalled();
  });

  it("starts nothing in the browser", async () => {
    stubServer();
    renderScreen();
    await settle();

    expect(screen.getByRole("tab", { name: "Login" })).toBeInTheDocument();
    expect(startLocalServer).not.toHaveBeenCalled();
  });

  it("says the app's own Message Crate is starting, in place of Connected", async () => {
    tauriState.isTauri = true;
    startLocalServer.mockResolvedValue({ status: "starting", first_time: false });
    invokeLocalServerStatus.mockResolvedValue({ status: "starting", first_time: false });
    stubNoServer();
    renderScreen();
    await settle();

    expect(screen.getByRole("status")).toHaveTextContent("Starting Message Crate…");
    expect(screen.queryByRole("tab", { name: "Login" })).toBeNull();
  });

  it("says a first start is setting Message Crate up", async () => {
    tauriState.isTauri = true;
    startLocalServer.mockResolvedValue({ status: "starting", first_time: true });
    invokeLocalServerStatus.mockResolvedValue({ status: "starting", first_time: true });
    stubNoServer();
    renderScreen();
    await settle();

    expect(screen.getByRole("status")).toHaveTextContent(
      "Setting up Message Crate for the first time…",
    );
  });

  it("connects as soon as the app's own Message Crate answers", async () => {
    tauriState.isTauri = true;
    startLocalServer.mockResolvedValue({ status: "starting", first_time: true });
    invokeLocalServerStatus.mockResolvedValue({ status: "starting", first_time: true });
    stubNoServer();
    renderScreen();
    await settle();
    expect(screen.getByText("Setting up Message Crate for the first time…")).toBeInTheDocument();

    stubServer("unclaimed", true);
    invokeLocalServerStatus.mockResolvedValue({ status: "ready", started_by_app: true });

    // The card hears the app's server is ready when it next asks.
    await advanceTimersAndSettle(STARTING_POLL_MS - 1);
    expect(screen.getByRole("status")).toHaveTextContent("Setting up Message Crate");
    await advanceTimersAndSettle(1);
    expect(screen.getByRole("status")).toHaveTextContent("Connected");
    expect(screen.getByRole("button", { name: "Explore Demo Account" })).toBeEnabled();
  });

  it("says why its own Message Crate did not start, and tries again when asked", async () => {
    tauriState.isTauri = true;
    startLocalServer.mockResolvedValue({
      status: "failed",
      reason: "port_taken",
      message: "Another program is using port 8080. Close it, or enter another server address.",
      details: "",
    });
    stubNoServer();
    const user = setupUser();
    renderScreen();
    await settle();

    expect(screen.getByRole("alert")).toHaveTextContent("Another program is using port 8080.");
    expect(screen.getByRole("button", { name: "Change server address" })).toBeEnabled();
    // Nothing the server wrote, so nothing to disclose.
    expect(screen.queryByText("Details")).toBeNull();

    await user.click(screen.getByRole("button", { name: "Open data directory" }));
    expect(invokeOpenDataDirectory).toHaveBeenCalledTimes(1);

    await user.click(screen.getByRole("button", { name: "Try again" }));
    expect(startLocalServer).toHaveBeenCalledTimes(2);
  });

  it("keeps the server's own words under Details", async () => {
    tauriState.isTauri = true;
    startLocalServer.mockResolvedValue({
      status: "failed",
      reason: "start_failed",
      message: "Message Crate stopped while starting.",
      details: "Error: database is locked",
    });
    stubNoServer();
    renderScreen();
    await settle();

    expect(screen.getByText("Details")).toBeInTheDocument();
    expect(screen.getByText("Error: database is locked")).toBeInTheDocument();
  });

  it("opens the desktop app on the login card for a saved address elsewhere, and starts nothing", async () => {
    tauriState.isTauri = true;
    authState.serverUrl = "http://crate.example:8080";
    stubServer();
    renderScreen();
    await settle();

    expect(screen.getByRole("tab", { name: "Login" })).toBeInTheDocument();
    expect(screen.getByText("Connected to crate.example:8080")).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Server Address" })).toBeNull();
    expect(startLocalServer).not.toHaveBeenCalled();
  });

  it("starts the app's own Message Crate when asked to go back to it", async () => {
    // The person left the app on a Message Crate elsewhere, so the app started
    // nothing at launch, and nothing answers at the app's own address until
    // the app starts its server.
    tauriState.isTauri = true;
    authState.serverUrl = "http://crate.example:8080";
    let ownRunning = false;
    startLocalServer.mockImplementation(async () => {
      ownRunning = true;
      return { status: "starting", first_time: false };
    });
    invokeLocalServerStatus.mockResolvedValue({ status: "ready", started_by_app: true });
    serveAt((address) =>
      ownRunning && address.startsWith("http://127.0.0.1:8080") ? { state: "closed" } : "down",
    );
    const user = setupUser();
    renderScreen();
    await settle();

    await user.click(screen.getByRole("button", { name: "Change server address" }));
    await user.click(
      screen.getByRole("button", { name: "Use the Message Crate on this computer" }),
    );
    await settle();

    expect(startLocalServer).toHaveBeenCalledTimes(1);
    // The app's server answers only once it has started, which the card hears
    // when it next asks how the start is going.
    expect(setServer).not.toHaveBeenCalledWith("http://127.0.0.1:8080");
    await advanceTimersAndSettle(STARTING_POLL_MS);
    expect(setServer).toHaveBeenCalledWith("http://127.0.0.1:8080");
    expect(screen.getByText(/^Connected to /)).toBeInTheDocument();
    expect(setBaseUrlSpy).not.toHaveBeenCalledWith("http://crate.example:8080");
  });

  it("tries a login saved for the app's own address once the app is back on it", async () => {
    // The saved address is elsewhere. The saved login was made at the app's
    // own. Going back must take the person in without asking for the
    // password the app already holds a token for. The connect on mount
    // leaves the saved login alone: the startup check already judged it.
    tauriState.isTauri = true;
    authState.serverUrl = "http://crate.example:8080";
    stubServer();
    const user = setupUser();
    renderScreen();
    await settle();

    expect(screen.getByText("Connected to crate.example:8080")).toBeInTheDocument();
    expect(retrySavedLogin).not.toHaveBeenCalled();

    await user.click(screen.getByRole("button", { name: "Change server address" }));
    await user.click(
      screen.getByRole("button", { name: "Use the Message Crate on this computer" }),
    );
    await settle();

    expect(retrySavedLogin).toHaveBeenCalledWith("http://127.0.0.1:8080");
  });

  it("says why the app's own Message Crate did not start, when asked to go back to it", async () => {
    tauriState.isTauri = true;
    authState.serverUrl = "http://crate.example:8080";
    startLocalServer.mockResolvedValue({
      status: "failed",
      reason: "port_taken",
      message: "Another program is using port 8080.",
      details: "",
    });
    stubNoServer();
    const user = setupUser();
    renderScreen();
    await settle();

    await user.click(screen.getByRole("button", { name: "Change server address" }));
    await user.click(
      screen.getByRole("button", { name: "Use the Message Crate on this computer" }),
    );
    await settle();

    expect(screen.getByRole("alert")).toHaveTextContent("Another program is using port 8080.");
    expect(setServer).not.toHaveBeenCalledWith("http://127.0.0.1:8080");
  });

  it("opens the browser on the login card whatever the address", async () => {
    authState.serverUrl = "http://crate.example:8080";
    stubServer();
    renderScreen();
    await settle();

    expect(screen.getByRole("tab", { name: "Login" })).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Server Address" })).toBeNull();
  });

  it("offers the website's own Message Crate as the way back from an address elsewhere", async () => {
    // The saved address outlives logout, so a website once pointed at a
    // Message Crate that has since gone away needs a way back to its own.
    authState.serverUrl = "http://old.example:8080";
    serveAt((address) => (address === "" ? { state: "open" } : "down"));
    const user = setupUser();
    renderScreen();
    await settle();

    expect(screen.getByText(/^Disconnected from /)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Change server address" }));
    await user.click(screen.getByRole("button", { name: "Use this website's own Message Crate" }));
    await settle();

    expect(setServer).toHaveBeenCalledWith("");
    expect(screen.getByText(/^Connected to /)).toBeInTheDocument();
    expect(startLocalServer).not.toHaveBeenCalled();
  });

  it("offers no way back while the desktop app is on its own Message Crate, slash or not", async () => {
    tauriState.isTauri = true;
    authState.serverUrl = "http://127.0.0.1:8080/";
    stubServer();
    const user = setupUser();
    renderScreen();
    await settle();

    expect(screen.getByText(/^Connected to /)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Change server address" }));

    expect(screen.getByRole("heading", { name: "Server Address" })).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Use the Message Crate on this computer" }),
    ).toBeNull();
  });

  it("offers no way back while the website is on its own Message Crate", async () => {
    stubServer();
    const user = setupUser();
    renderScreen();
    await settle();

    expect(screen.getByText(/^Connected to /)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Change server address" }));

    expect(screen.getByRole("heading", { name: "Server Address" })).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Use this website's own Message Crate" }),
    ).toBeNull();
    expect(
      screen.queryByRole("button", { name: "Use the Message Crate on this computer" }),
    ).toBeNull();
  });
});
