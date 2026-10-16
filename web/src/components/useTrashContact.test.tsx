/** @vitest-environment jsdom */

/**
 * What moving a contact to the Trash sends, and what it leaves marked stale.
 *
 * Every write marks the whole account's cache stale. The entries named here
 * are the ones a screen showed out of date when a write left them out, so a
 * regression names the screen it breaks.
 */

import { QueryClientProvider } from "@tanstack/react-query";
import { renderHook } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { keys } from "../lib/queryKeys";
import { trashContact as trashContactRoute } from "../lib/serverApi";
import { authModule, testQueryClient } from "../test/providers";
import { freshEntries, seedEntries } from "../test/staleEntries";
import { useTrashContact } from "./useTrashContact";

vi.mock("../hooks/useAuth", () => authModule({ accountId: 7 }));

vi.mock("../lib/serverApi", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/serverApi")>()),
  trashContact: vi.fn(),
}));

const trashContact = vi.mocked(trashContactRoute);

let client = testQueryClient();

function wrapper({ children }: { children: ReactNode }) {
  return <QueryClientProvider client={client}>{children}</QueryClientProvider>;
}

beforeEach(() => {
  vi.clearAllMocks();
  client = testQueryClient();
});

describe("useTrashContact", () => {
  const shown = [keys.contacts.lists, keys.contacts.detail(9)];

  it("marks the contacts list and the contact's own detail stale", async () => {
    trashContact.mockResolvedValue(undefined);
    const { result } = renderHook(() => useTrashContact(), { wrapper });

    seedEntries(client, 7, shown);
    await result.current.mutateAsync(9);

    expect(trashContact).toHaveBeenCalledWith(9, expect.anything());
    expect(freshEntries(client, 7, shown)).toEqual([]);
  });
});
