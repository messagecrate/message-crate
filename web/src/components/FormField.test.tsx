/** @vitest-environment jsdom */

import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import FormField from "./FormField";

describe("FormField", () => {
  it("renders an inline label and control", () => {
    render(
      <FormField label="Source">
        <input aria-label="source-input" />
      </FormField>,
    );
    expect(screen.getByText("Source")).toBeInTheDocument();
    expect(screen.getByLabelText("source-input")).toBeInTheDocument();
  });

  it("renders stacked layout with optional trailing", () => {
    render(
      <FormField label="Password" layout="stacked" trailing={<span>show</span>}>
        <input aria-label="password-input" />
      </FormField>,
    );
    expect(screen.getByText("Password")).toBeInTheDocument();
    expect(screen.getByText("show")).toBeInTheDocument();
    expect(screen.getByLabelText("password-input")).toBeInTheDocument();
  });

  it("associates a stacked label with the first control when hints follow it", () => {
    render(
      <FormField label="Attachment directory" layout="stacked">
        <input />
        <p>Directory that contains Attachments and StickerCache.</p>
      </FormField>,
    );
    expect(screen.getByLabelText("Attachment directory")).toBeTruthy();
  });

  it("marks a required stacked label with a red asterisk", () => {
    render(
      <FormField label="iPhone Backup Directory" layout="stacked" required>
        <input />
      </FormField>,
    );
    const label = screen.getByText("iPhone Backup Directory").closest("label");
    expect(label?.textContent).toContain("*");
    expect(label?.querySelector("[aria-hidden]")?.className).toContain("text-danger");
  });

  it("marks an optional stacked label with (Optional)", () => {
    render(
      <FormField label="Apple Contacts file" layout="stacked" optional>
        <input />
      </FormField>,
    );
    expect(screen.getByLabelText("Apple Contacts file (Optional)")).toBeTruthy();
  });
});
