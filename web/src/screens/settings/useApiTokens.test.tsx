/** @vitest-environment jsdom */

/**
 * What a token write leaves behind.
 *
 * The hook used to call `refetch` on its own query after each write, which
 * refreshed the list this hook holds and nothing else. It marks the list stale
 * instead, so anything showing tokens refreshes.
 */

import { type QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, screen } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ApiTokenRevealProvider } from "../../components/ApiTokenRevealDialog";
import { createApiToken, deleteApiToken, listApiTokens, renameApiToken } from "../../lib/serverApi";
import type { components } from "../../lib/serverApi.types";
import { deferred } from "../../test/deferred";
import { settle, startFakeClock, stopFakeClock } from "../../test/fakeTimers";
import { authModule, testQueryClient } from "../../test/providers";
import { useApiTokens } from "./useApiTokens";

type ApiToken = components["schemas"]["ApiToken"];

vi.mock("../../lib/authContext", () => authModule({ accountId: 7 }));

vi.mock("../../lib/serverApi", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/serverApi")>()),
  listApiTokens: vi.fn(),
  createApiToken: vi.fn(),
  renameApiToken: vi.fn(),
  deleteApiToken: vi.fn(),
}));

const list = vi.mocked(listApiTokens);
const create = vi.mocked(createApiToken);
const rename = vi.mocked(renameApiToken);
const revoke = vi.mocked(deleteApiToken);

let client: QueryClient;

function wrapper({ children }: { children: ReactNode }) {
  return (
    <QueryClientProvider client={client}>
      <ApiTokenRevealProvider>{children}</ApiTokenRevealProvider>
    </QueryClientProvider>
  );
}

const token: ApiToken = {
  id: 1,
  label: "Laptop",
  can_import: true,
  can_export: true,
  created_at: "1700000000",
  token_hint: "mc-api-la..op",
  last_accessed_at: null,
  expires_at: null,
  disabled: false,
};

beforeEach(() => {
  startFakeClock();
  vi.clearAllMocks();
  list.mockResolvedValue([token]);
  client = testQueryClient();
});

afterEach(() => {
  stopFakeClock();
});

describe("useApiTokens", () => {
  it("asks for the list again after a token is created", async () => {
    create.mockResolvedValue({ ...token, token: "mc-api-secret" } as unknown as Awaited<
      ReturnType<typeof createApiToken>
    >);
    const { result } = renderHook(() => useApiTokens(), { wrapper });
    await settle();
    expect(result.current.loading).toBe(false);
    expect(list).toHaveBeenCalledTimes(1);

    act(() => {
      result.current.setLabel("Laptop");
    });
    act(() => {
      result.current.create();
    });

    await settle();
    expect(create).toHaveBeenCalledWith({
      label: "Laptop",
      can_import: true,
      can_export: true,
    });
    expect(list).toHaveBeenCalledTimes(2);
    expect(screen.getByText("mc-api-secret")).toBeInTheDocument();
  });

  it("asks for the list again after a token is revoked, and closes the dialog either way", async () => {
    revoke.mockRejectedValue(new Error("already revoked"));
    const { result } = renderHook(() => useApiTokens(), { wrapper });
    await settle();
    expect(result.current.loading).toBe(false);

    act(() => {
      result.current.revoke(token);
    });
    await settle();

    expect(result.current.actionError).toBe("Already revoked");
    expect(result.current.revokeTarget).toBeNull();
    expect(list).toHaveBeenCalledTimes(2);

    act(() => {
      result.current.cancelCompose();
    });
    await settle();
    expect(result.current.actionError).toBe("");
  });

  it("reports a write in flight while a rename is unanswered", async () => {
    const answer = deferred<Awaited<ReturnType<typeof renameApiToken>>>();
    rename.mockReturnValue(answer.promise);
    const { result } = renderHook(() => useApiTokens(), { wrapper });
    await settle();
    expect(result.current.loading).toBe(false);

    act(() => {
      result.current.openRename(token);
    });
    act(() => {
      result.current.setRenameLabel("Desktop");
    });
    act(() => {
      result.current.rename();
    });

    await settle();
    expect(result.current.busy).toBe(true);
    answer.resolve({} as never);
    await settle();
    expect(result.current.busy).toBe(false);
    expect(rename).toHaveBeenCalledWith(1, { label: "Desktop" });
    expect(result.current.renameTarget).toBeNull();
  });

  it("calls them API Tokens when the list cannot be loaded", async () => {
    list.mockRejectedValue(new Error(""));
    const { result } = renderHook(() => useApiTokens(), { wrapper });
    await settle();
    expect(result.current.loadError).toBe("Could not load API Tokens.");
  });
});
