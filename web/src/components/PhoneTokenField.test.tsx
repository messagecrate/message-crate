/** @vitest-environment jsdom */

import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { focusRing } from "../lib/uiStyles";
import PhoneTokenField from "./PhoneTokenField";

afterEach(cleanup);

describe("PhoneTokenField focus", () => {
  // The field's border shows that something inside it has focus but not which
  // number, so each number draws the shared focus ring itself.
  it("draws the focus ring on a number reached with the keyboard", () => {
    render(
      <PhoneTokenField value={["555-0101", "555-0102"]} onChange={() => {}} aria-label="Phones" />,
    );
    const numbers = screen.getAllByRole("row");
    expect(numbers).toHaveLength(2);
    for (const number of numbers) {
      expect(number.className).toContain(focusRing);
    }
  });
});
