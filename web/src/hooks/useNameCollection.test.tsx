/** @vitest-environment jsdom */

import { type QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createNameCollection } from "../lib/nameCollection";
import { keys } from "../lib/queryKeys";
import { settle, startFakeClock, stopFakeClock } from "../test/fakeTimers";
import { authModule, testQueryClient } from "../test/providers";
import { useNameCollection } from "./useNameCollection";

vi.mock("./useAuth", () => authModule({ accountId: 7 }));

let client: QueryClient;

function wrapper({ children }: { children: ReactNode }) {
  return <QueryClientProvider client={client}>{children}</QueryClientProvider>;
}

beforeEach(() => {
  startFakeClock();
  client = testQueryClient();
});

afterEach(() => {
  stopFakeClock();
});

describe("useNameCollection", () => {
  it("answers the names in the server's order", async () => {
    const groups = createNameCollection({
      routes: {
        list: vi.fn().mockResolvedValue([
          { id: 2, name: "Family" },
          { id: 1, name: "Work" },
        ]),
        create: vi.fn(),
        update: vi.fn(),
        remove: vi.fn(),
        updateMembers: vi.fn(),
      },
      key: keys.contactGroups.all,
      chips: [],
      label: "group",
      forName: (name) => `group:${name}`,
      reserved: { label: "Contact Group", reserved: [], special: [] },
    });
    const { result } = renderHook(() => useNameCollection(groups), { wrapper });
    await settle();
    expect(result.current.loading).toBe(false);
    expect(result.current.names).toEqual(["Family", "Work"]);
  });
});
