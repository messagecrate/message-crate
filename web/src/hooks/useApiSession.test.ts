/** @vitest-environment jsdom */

import { renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ApiSession } from "../lib/apiSession";
import { useApiSession } from "./useApiSession";

const setBaseUrl = vi.fn();
const setToken = vi.fn();
const setAccountId = vi.fn();

vi.mock("../lib/api", () => ({
  setBaseUrl: (url: string) => setBaseUrl(url),
  setToken: (token: string | null) => setToken(token),
  setAccountId: (id: number | null) => setAccountId(id),
}));

beforeEach(() => {
  setBaseUrl.mockReset();
  setToken.mockReset();
  setAccountId.mockReset();
});

describe("useApiSession", () => {
  it("follows the login as it changes", () => {
    const loggedIn: ApiSession = { serverUrl: "http://127.0.0.1:8080", token: "old", accountId: 7 };
    const { rerender } = renderHook((session: ApiSession) => useApiSession(session), {
      initialProps: loggedIn,
    });
    expect(setToken).toHaveBeenLastCalledWith("old");
    expect(setAccountId).toHaveBeenLastCalledWith(7);

    rerender({ serverUrl: "http://127.0.0.1:8080", token: null, accountId: null });

    expect(setToken).toHaveBeenLastCalledWith(null);
    expect(setAccountId).toHaveBeenLastCalledWith(null);
  });
});
