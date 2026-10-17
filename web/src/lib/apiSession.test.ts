/** @vitest-environment jsdom */

import { beforeEach, describe, expect, it, vi } from "vitest";
import { applyApiSession } from "./apiSession";

const setBaseUrl = vi.fn();
const setToken = vi.fn();
const setAccountId = vi.fn();

vi.mock("./api", () => ({
  setBaseUrl: (url: string) => setBaseUrl(url),
  setToken: (token: string | null) => setToken(token),
  setAccountId: (id: number | null) => setAccountId(id),
}));

beforeEach(() => {
  setBaseUrl.mockReset();
  setToken.mockReset();
  setAccountId.mockReset();
});

describe("applyApiSession", () => {
  it("points api.ts at every field given", () => {
    applyApiSession({ serverUrl: "http://127.0.0.1:8080", token: "t", accountId: 7 });

    expect(setBaseUrl).toHaveBeenCalledWith("http://127.0.0.1:8080");
    expect(setToken).toHaveBeenCalledWith("t");
    expect(setAccountId).toHaveBeenCalledWith(7);
  });

  it("clears the token and account id without moving the server address", () => {
    applyApiSession({ token: null, accountId: null });

    expect(setToken).toHaveBeenCalledWith(null);
    expect(setAccountId).toHaveBeenCalledWith(null);
    expect(setBaseUrl).not.toHaveBeenCalled();
  });

  it("sets the empty server address, which means the page's own host", () => {
    applyApiSession({ serverUrl: "" });

    expect(setBaseUrl).toHaveBeenCalledWith("");
    expect(setToken).not.toHaveBeenCalled();
    expect(setAccountId).not.toHaveBeenCalled();
  });
});
