/** @vitest-environment jsdom */

import { cleanup, render } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { removeIdentityConfirmBody } from "./handleTableLogic";

afterEach(() => {
  cleanup();
});

function bodyText(conversationCount: number): string {
  const { container } = render(
    removeIdentityConfirmBody({
      address: "+15555550100",
      service: "sms",
      serviceLabel: "Text Message",
      conversationCount,
    }),
  );
  return container.textContent ?? "";
}

describe("removeIdentityConfirmBody", () => {
  it("writes the conversation count with a thousands separator", () => {
    expect(bodyText(1234)).toContain(
      `will unlink ${(1234).toLocaleString()} conversations from this contact`,
    );
  });

  it("writes one conversation in the singular", () => {
    expect(bodyText(1)).toContain("will unlink 1 conversation from this contact");
  });
});
