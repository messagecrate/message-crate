/** @vitest-environment jsdom */

import { beforeEach, describe, expect, it, vi } from "vitest";
import { ApiError } from "./api";
import {
  checkSavedLogin,
  forgetSavedLogin,
  loadSavedLogin,
  loadServerAddress,
  saveLogin,
  saveServerAddress,
  startingAuthState,
} from "./savedLogin";

const getSession = vi.fn();
const setToken = vi.fn();

vi.mock("./api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./api")>()),
  setToken: (token: string | null) => setToken(token),
}));

vi.mock("./serverApi", () => ({
  getSession: () => getSession(),
}));

const HERE = "http://127.0.0.1:8080";
const ELSEWHERE = "http://crate.example:8080";

function saveLoginFor(serverUrl: string) {
  saveLogin({ serverUrl, token: "session-token", accountId: 7, isAuthenticated: true });
}

describe("saved login", () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it("reads back the login it saved", () => {
    saveLoginFor(HERE);

    expect(loadSavedLogin()).toEqual({ serverUrl: HERE, token: "session-token", accountId: 7 });
  });

  it("keeps the server address when the login is forgotten", () => {
    saveServerAddress(ELSEWHERE);
    saveLoginFor(ELSEWHERE);

    forgetSavedLogin();

    expect(loadSavedLogin()).toBeNull();
    expect(loadServerAddress()).toBe(ELSEWHERE);
  });

  it("starts logged out when nothing is saved", () => {
    expect(startingAuthState()).toEqual({
      serverUrl: "",
      token: null,
      accountId: null,
      isAuthenticated: false,
    });
  });

  it("starts logged in with a login saved for the saved server address", () => {
    saveServerAddress(HERE);
    saveLoginFor(HERE);

    expect(startingAuthState()).toEqual({
      serverUrl: HERE,
      token: "session-token",
      accountId: 7,
      isAuthenticated: true,
    });
  });

  it("starts logged out at the saved address when the login is for another one", () => {
    saveServerAddress(HERE);
    saveLoginFor(ELSEWHERE);

    expect(startingAuthState()).toEqual({
      serverUrl: HERE,
      token: null,
      accountId: null,
      isAuthenticated: false,
    });
    expect(loadSavedLogin()?.serverUrl).toBe(ELSEWHERE);
  });
});

describe("checkSavedLogin", () => {
  const LOGIN = { serverUrl: HERE, token: "session-token", accountId: 7 };

  beforeEach(() => {
    getSession.mockReset();
    setToken.mockReset();
  });

  it("asks with the login it checks", async () => {
    getSession.mockImplementation(async () => {
      expect(setToken).toHaveBeenLastCalledWith("session-token");
      return { account_id: 7 };
    });
    expect(await checkSavedLogin(LOGIN)).toBe("valid");
    expect(getSession).toHaveBeenCalledTimes(1);
  });

  it("is valid when the server accepts the session", async () => {
    getSession.mockResolvedValue({ account_id: 7 });
    expect(await checkSavedLogin(LOGIN)).toBe("valid");
  });

  it("is refused when the server answers 401", async () => {
    getSession.mockRejectedValue(new ApiError(401, "Unauthorized"));
    expect(await checkSavedLogin(LOGIN)).toBe("refused");
  });

  it("is unanswered when something other than the server answers", async () => {
    getSession.mockRejectedValue(new ApiError(502, "Bad Gateway"));
    expect(await checkSavedLogin(LOGIN)).toBe("unanswered");
  });

  it("is unanswered when nothing answers", async () => {
    getSession.mockRejectedValue(new TypeError("Failed to fetch"));
    expect(await checkSavedLogin(LOGIN)).toBe("unanswered");
  });
});
