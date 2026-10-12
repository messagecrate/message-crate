/** @vitest-environment jsdom */

import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import AuthErrorFooter from "./AuthErrorFooter";

afterEach(cleanup);

describe("AuthErrorFooter", () => {
  it("shows the message as its caller gives it", () => {
    render(<AuthErrorFooter error="Passwords do not match." />);
    expect(screen.getByText("Passwords do not match.")).toBeInTheDocument();
  });

  it("reserves its space when there is no message", () => {
    const { container } = render(<AuthErrorFooter error="" />);
    const line = container.firstElementChild;
    expect(line).toHaveClass("h-9");
    expect(line).toHaveAttribute("aria-live", "polite");
  });

  it("settles the message on the bottom edge, so it grows upward", () => {
    const { container } = render(<AuthErrorFooter error="something went wrong" />);
    const band = container.firstElementChild;
    expect(band).toHaveClass("flex", "flex-col");
    // `mt-auto` and not `justify-end`: bottom-aligned, but still scrollable
    // when the message is longer than the band.
    expect(band?.firstElementChild).toHaveClass("mt-auto");
  });

  it("takes a taller band where there is room for one", () => {
    const { container } = render(<AuthErrorFooter error="" className="h-16" />);
    const band = container.firstElementChild;
    expect(band).toHaveClass("h-16");
    expect(band).not.toHaveClass("h-9");
  });
});
