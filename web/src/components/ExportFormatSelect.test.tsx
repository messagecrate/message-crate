/** @vitest-environment jsdom */

import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { EXPORT_FORMATS } from "../lib/tauri";
import { setupUser } from "../test/user";
import ExportFormatSelect from "./ExportFormatSelect";

describe("ExportFormatSelect", () => {
  afterEach(() => {
    cleanup();
  });

  it("shows the label it is announced by", () => {
    render(<ExportFormatSelect value="jsonl" onChange={vi.fn()} />);

    expect(screen.getByText("Output format")).toBeTruthy();
    expect(screen.getByRole("button", { name: /Output format/ })).toBeTruthy();
  });

  it("offers every export format and reports the one picked by its id", async () => {
    const user = setupUser();
    const onChange = vi.fn();
    render(<ExportFormatSelect value="jsonl" onChange={onChange} />);

    await user.click(screen.getByRole("button", { name: /Output format/ }));
    const offered = (await screen.findAllByRole("option")).map((o) => o.textContent);
    expect(offered).toEqual(EXPORT_FORMATS.map((f) => f.label));

    await user.click(screen.getByRole("option", { name: "CSV (.csv)" }));
    expect(onChange).toHaveBeenCalledWith("csv");
  });

  it("cannot be opened while disabled", () => {
    render(<ExportFormatSelect value="jsonl" onChange={vi.fn()} isDisabled />);

    expect(screen.getByRole("button", { name: /Output format/ })).toBeDisabled();
  });
});
