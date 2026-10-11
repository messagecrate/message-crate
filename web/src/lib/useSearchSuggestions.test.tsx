/** @vitest-environment jsdom */

import { type QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createQueryClient } from "./routeQuery";
import type { SearchField } from "./searchFields";
import { listContacts, listSearchFields } from "./serverApi";
import {
  applySuggestionToQuery,
  buildSearchSuggestions,
  CONTACT_SUGGESTION_DEBOUNCE_MS,
  useSearchSuggestions,
} from "./useSearchSuggestions";

vi.mock("./authContext", () => ({ useAuth: () => ({ accountId: 7 }) }));

vi.mock("./serverApi", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./serverApi")>()),
  listContacts: vi.fn(),
  listSearchFields: vi.fn(),
}));

const fields: SearchField[] = [
  { word: "with", value_type: "person", values: [], help: "", example: "" },
  { word: "from", value_type: "person", values: [], help: "", example: "" },
  { word: "tag", value_type: "name", values: ["none"], help: "", example: "" },
  { word: "kind", value_type: "choice", values: ["direct", "group"], help: "", example: "" },
];

describe("buildSearchSuggestions", () => {
  it("completes a bare prefix to the words the list has", () => {
    const out = buildSearchSuggestions({
      completingValue: false,
      personOp: false,
      lastToken: "t",
      fields: [...fields],
      contacts: [],
    });
    expect(out.map((s) => s.insert)).toEqual(["tag:"]);
  });
  it("offers a choice word's values after the colon", () => {
    const out = buildSearchSuggestions({
      completingValue: true,
      personOp: false,
      lastToken: "kind:",
      fields: [...fields],
      contacts: [],
    });
    expect(out.map((s) => s.insert)).toEqual(["kind:direct ", "kind:group "]);
  });
  it("offers contacts by id for a person word", () => {
    const out = buildSearchSuggestions({
      completingValue: true,
      personOp: true,
      lastToken: "with:ja",
      fields: [...fields],
      contacts: [{ id: "42", name: "Jane Doe" }],
    });
    expect(out[0].insert).toBe("with:#42 ");
    expect(out[0].label).toBe("Jane Doe");
  });
});

describe("negated terms", () => {
  it("keeps the minus of a negated person term", () => {
    const [first] = buildSearchSuggestions({
      completingValue: true,
      personOp: true,
      lastToken: "-with:ann",
      fields: [...fields],
      contacts: [{ id: "42", name: "Ann Lee" }],
    });
    expect(applySuggestionToQuery("-with:ann", first)).toBe("-with:#42 ");
  });
  it("keeps the minus of a negated choice term", () => {
    const [first] = buildSearchSuggestions({
      completingValue: true,
      personOp: false,
      lastToken: "-kind:gr",
      fields: [...fields],
      contacts: [],
    });
    expect(applySuggestionToQuery("-kind:gr", first)).toBe("-kind:group ");
  });
  it("keeps the minus of a negated word", () => {
    const [first] = buildSearchSuggestions({
      completingValue: false,
      personOp: false,
      lastToken: "-ta",
      fields: [...fields],
      contacts: [],
    });
    expect(applySuggestionToQuery("a -ta", first)).toBe("a -tag:");
  });
});

describe("applySuggestionToQuery", () => {
  it("leaves the spaces inside a quoted phrase as typed", () => {
    expect(
      applySuggestionToQuery('subject:"a  b" wi', { id: "with", label: "with:", insert: "with:" }),
    ).toBe('subject:"a  b" with:');
  });

  it("replaces the token being typed", () => {
    expect(applySuggestionToQuery("hello ta", { id: "tag", label: "tag:", insert: "tag:" })).toBe(
      "hello tag:",
    );
  });
});

describe("useSearchSuggestions", () => {
  const contacts = vi.mocked(listContacts);
  let client: QueryClient;

  function wrapper({ children }: { children: ReactNode }) {
    return <QueryClientProvider client={client}>{children}</QueryClientProvider>;
  }

  /** The contacts the server has; it answers those whose name starts with `q`. */
  const people = [
    { id: 1, name: "Jane Doe" },
    { id: 2, name: "Jo Park" },
  ];

  beforeEach(() => {
    vi.useFakeTimers();
    // The app's own client, so an answer stays fresh as long as it does on screen.
    client = createQueryClient();
    vi.mocked(listSearchFields).mockResolvedValue([...fields]);
    contacts.mockReset();
    contacts.mockImplementation(async ({ q }) => {
      const items = people.filter((p) => p.name.toLowerCase().startsWith((q ?? "").toLowerCase()));
      return { items, total: items.length } as unknown as Awaited<ReturnType<typeof listContacts>>;
    });
  });

  afterEach(() => {
    client.clear();
    vi.useRealTimers();
  });

  /** Let timers up to `ms` fire, and every answer they start land. */
  async function wait(ms: number) {
    await act(async () => {
      await vi.advanceTimersByTimeAsync(ms);
    });
    // The request a timer started, and the render its answer causes.
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });
  }

  function typing(first: string) {
    return renderHook(({ value }) => useSearchSuggestions(value, "conversations"), {
      wrapper,
      initialProps: { value: first },
    });
  }

  it("asks for contacts once, after the person stops typing a person word", async () => {
    const hook = typing("with:");
    await wait(0);
    hook.rerender({ value: "with:j" });
    await wait(CONTACT_SUGGESTION_DEBOUNCE_MS - 1);
    hook.rerender({ value: "with:ja" });
    await wait(CONTACT_SUGGESTION_DEBOUNCE_MS - 1);
    expect(contacts).not.toHaveBeenCalled();

    await wait(1);
    expect(contacts).toHaveBeenCalledTimes(1);
    expect(contacts.mock.calls[0][0]).toEqual({ q: "ja", limit: 20, offset: 0 });
    expect(hook.result.current.map((s) => s.label)).toEqual(["Jane Doe"]);
  });

  it("answers a prefix typed again from the cache", async () => {
    const hook = typing("with:ja");
    await wait(CONTACT_SUGGESTION_DEBOUNCE_MS);
    hook.rerender({ value: "with:j" });
    await wait(CONTACT_SUGGESTION_DEBOUNCE_MS);
    hook.rerender({ value: "with:ja" });
    await wait(CONTACT_SUGGESTION_DEBOUNCE_MS);

    expect(contacts.mock.calls.map(([params]) => params.q)).toEqual(["ja", "j"]);
    expect(hook.result.current.map((s) => s.label)).toEqual(["Jane Doe"]);
  });

  it("keeps the last contacts on screen while the next prefix waits", async () => {
    const hook = typing("with:j");
    await wait(CONTACT_SUGGESTION_DEBOUNCE_MS);
    expect(hook.result.current.map((s) => s.label)).toEqual(["Jane Doe", "Jo Park"]);

    hook.rerender({ value: "with:ja" });
    await wait(CONTACT_SUGGESTION_DEBOUNCE_MS - 1);
    expect(hook.result.current.map((s) => s.label)).toEqual(["Jane Doe", "Jo Park"]);
  });

  it("asks for no contacts for a word whose value is not a person", async () => {
    const hook = typing("kind:gr");
    await wait(CONTACT_SUGGESTION_DEBOUNCE_MS);
    expect(contacts).not.toHaveBeenCalled();
    expect(hook.result.current.map((s) => s.insert)).toEqual(["kind:group "]);
  });

  it("does not ask for contacts with the value of the word typed before", async () => {
    const hook = typing("kind:gr");
    await wait(CONTACT_SUGGESTION_DEBOUNCE_MS);
    hook.rerender({ value: "kind:gr with:" });
    await wait(CONTACT_SUGGESTION_DEBOUNCE_MS);

    expect(contacts.mock.calls.map(([params]) => params.q)).toEqual([""]);
  });

  /** Hold the answer for `q` back until the returned function is called. */
  function holdAnswerFor(q: string): () => void {
    let release = () => {};
    const answer = contacts.getMockImplementation();
    contacts.mockImplementation(async (params, opts) => {
      if (params.q === q) await new Promise<void>((resolve) => (release = resolve));
      return answer?.(params, opts) as ReturnType<typeof listContacts>;
    });
    return () => release();
  }

  it("does not offer the names typed for another person word", async () => {
    const hook = typing("from:jo");
    await wait(CONTACT_SUGGESTION_DEBOUNCE_MS);
    expect(hook.result.current.map((s) => s.label)).toEqual(["Jo Park"]);

    const release = holdAnswerFor("");
    hook.rerender({ value: "from:jo with:" });
    expect(hook.result.current).toEqual([]);
    await wait(CONTACT_SUGGESTION_DEBOUNCE_MS);
    // Asked for, and not answered yet: Jo is not offered while it waits.
    expect(contacts.mock.calls.map(([params]) => params.q)).toEqual(["jo", ""]);
    expect(hook.result.current).toEqual([]);

    release();
    await wait(0);
    expect(hook.result.current.map((s) => s.label)).toEqual(["Jane Doe", "Jo Park"]);
  });

  it("starts a person word typed after a picked contact from no names", async () => {
    const hook = typing("with:jo");
    await wait(CONTACT_SUGGESTION_DEBOUNCE_MS);
    hook.rerender({ value: "with:#2 " });

    const release = holdAnswerFor("");
    hook.rerender({ value: "with:#2 with:" });
    expect(hook.result.current).toEqual([]);
    await wait(CONTACT_SUGGESTION_DEBOUNCE_MS);
    expect(hook.result.current).toEqual([]);
    release();
  });

  it("starts the same word typed again in a cleared box from no names", async () => {
    const hook = typing("with:jo");
    await wait(CONTACT_SUGGESTION_DEBOUNCE_MS);
    hook.rerender({ value: "" });
    await wait(CONTACT_SUGGESTION_DEBOUNCE_MS);

    const release = holdAnswerFor("");
    hook.rerender({ value: "with:" });
    await wait(CONTACT_SUGGESTION_DEBOUNCE_MS);
    expect(hook.result.current).toEqual([]);
    release();
  });
});
