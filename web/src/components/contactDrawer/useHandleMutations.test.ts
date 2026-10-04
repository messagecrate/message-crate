/** @vitest-environment jsdom */

import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ContactHandle } from "../../lib/contactDetail";
import { useHandleMutations } from "./useHandleMutations";

const mutate = vi.hoisted(() => vi.fn());
vi.mock("../../lib/contactDetail", () => ({
  useUpdateContact: () => ({ mutate, isPending: false, error: null }),
}));

/** One identity row as the server lists it for a contact. */
function row(address: string, service: string): ContactHandle {
  return {
    address,
    service,
    start_date: null,
    end_date: null,
    conversations: 0,
    direct_messages: 0,
    group_messages: 0,
  };
}

/** The `remove_identity` the hook sends for `handle`. */
function removalOf(handle: ContactHandle): unknown {
  const { result } = renderHook(() => useHandleMutations({ contactId: "7" }));
  act(() => result.current.requestRemoveHandle(handle));
  act(() => result.current.confirmRemoveHandle());
  return mutate.mock.calls[0][0].body.remove_identity;
}

beforeEach(() => mutate.mockReset());

describe("useHandleMutations", () => {
  it("removes a number on the service the list gives it", () => {
    expect(removalOf(row("+15555550100", "whatsapp"))).toEqual({
      address: "+15555550100",
      service: "whatsapp",
    });
  });

  it("names no service for an email address, which an import can store on WhatsApp", () => {
    // Sent as `phone`, an email address stored on WhatsApp was not found on
    // the contact and could not be removed.
    expect(removalOf(row("ann@example.com", "email"))).toEqual({
      address: "ann@example.com",
      service: undefined,
    });
  });

  it("names no service for a word the server does not take", () => {
    expect(removalOf(row("+15555550100", "sms"))).toEqual({
      address: "+15555550100",
      service: undefined,
    });
  });

  it("adds an email address on the phone service", () => {
    const { result } = renderHook(() => useHandleMutations({ contactId: "7" }));
    act(() => result.current.confirmAdd({ address: "ann@example.com", service: "email" }));
    expect(mutate.mock.calls[0][0].body.add_identity).toEqual({
      address: "ann@example.com",
      service: "phone",
    });
  });
});
