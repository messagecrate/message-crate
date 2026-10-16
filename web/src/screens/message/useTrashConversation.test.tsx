/** @vitest-environment jsdom */

/**
 * What moving a conversation to the Trash sends, and what it leaves marked
 * stale.
 *
 * Every write marks the whole account's cache stale. The entries named here
 * are the ones a screen showed out of date when a write left them out, so a
 * regression names the screen it breaks.
 */

import { QueryClientProvider } from "@tanstack/react-query";
import { renderHook } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { keys } from "../../lib/queryKeys";
import { trashConversation as trashConversationRoute } from "../../lib/serverApi";
import { authModule, testQueryClient } from "../../test/providers";
import { freshEntries, seedEntries } from "../../test/staleEntries";
import { useTrashConversation } from "./useTrashConversation";

vi.mock("../../hooks/useAuth", () => authModule({ accountId: 7 }));

vi.mock("../../lib/serverApi", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/serverApi")>()),
  trashConversation: vi.fn(),
}));

const trashConversation = vi.mocked(trashConversationRoute);

let client = testQueryClient();

function wrapper({ children }: { children: ReactNode }) {
  return <QueryClientProvider client={client}>{children}</QueryClientProvider>;
}

beforeEach(() => {
  vi.clearAllMocks();
  client = testQueryClient();
});

/** The conversation list, a find, the Trash count, and an open contact's counts. */
const conversationTrashState = [
  keys.conversations.lists,
  keys.conversations.find(42, "date:2020", "-date", 0, 50),
  keys.trash.all,
  keys.contacts.details,
];

describe("useTrashConversation", () => {
  it("marks every list and count that shows trash state stale", async () => {
    trashConversation.mockResolvedValue(undefined);
    const { result } = renderHook(() => useTrashConversation(), { wrapper });

    seedEntries(client, 7, conversationTrashState);
    await result.current.mutateAsync(42);

    expect(trashConversation).toHaveBeenCalledWith(42, expect.anything());
    expect(freshEntries(client, 7, conversationTrashState)).toEqual([]);
  });
});
