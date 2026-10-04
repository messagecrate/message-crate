/** @vitest-environment jsdom */

import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it } from "vitest";
import { focusRing } from "../lib/uiStyles";
import DateField from "./DateField";

afterEach(cleanup);

// Each button of the field sets `outline-none`, which removes the app's own
// :focus-visible outline, so it sets the outline back with `focusRing`.
describe("DateField", () => {
  it("draws the focus ring on the calendar button and the calendar's month buttons", async () => {
    render(<DateField label="From" value="2024-03-15" onChange={() => {}} />);

    const pick = screen.getByRole("button", { name: /Pick From/ });
    expect(pick.className).toContain(focusRing);

    await userEvent.click(pick);
    await screen.findAllByRole("button", { name: "Previous" });
    const months = document.querySelectorAll('button[slot="previous"], button[slot="next"]');
    expect(months).toHaveLength(2);
    for (const button of months) {
      expect(button.className).toContain(focusRing);
    }
  });
});
