/** @vitest-environment jsdom */

import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { focusRing } from "../../lib/uiStyles";
import { CollapsibleSection } from "./ImportFormUi";

afterEach(cleanup);

describe("CollapsibleSection", () => {
  // The disclosure sets `outline-none`, which removes the app's own
  // :focus-visible outline, so it sets the outline back with `focusRing`.
  it("draws the focus ring on its disclosure button", () => {
    render(
      <CollapsibleSection title="Attachments" open={false} onToggle={() => {}}>
        <p>Body</p>
      </CollapsibleSection>,
    );
    expect(screen.getByRole("button", { name: /Attachments/ }).className).toContain(focusRing);
  });
});
