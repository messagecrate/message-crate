/** @vitest-environment jsdom */

import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { setupUser } from "../test/user";
import OpenPathButton from "./OpenPathButton";

const openPathInExplorer = vi.fn();

vi.mock("../lib/openPath", () => ({
  openPathInExplorer: (...args: unknown[]) => openPathInExplorer(...args),
}));

describe("OpenPathButton", () => {
  afterEach(() => {
    cleanup();
  });

  beforeEach(() => {
    openPathInExplorer.mockReset();
  });

  it("opens the path when clicked", async () => {
    const user = setupUser();
    openPathInExplorer.mockResolvedValue(undefined);
    render(<OpenPathButton path="/home/sam/message-crate/staging">Open</OpenPathButton>);
    await user.click(screen.getByRole("button", { name: "Open" }));
    expect(openPathInExplorer).toHaveBeenCalledWith("/home/sam/message-crate/staging");
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("shows an alert when opening fails", async () => {
    const user = setupUser();
    openPathInExplorer.mockRejectedValue(
      new Error("Path is not in a directory Message Crate made in the Staging Directory"),
    );
    render(<OpenPathButton path="/tmp/nope">Open</OpenPathButton>);
    await user.click(screen.getByRole("button", { name: "Open" }));
    expect(screen.getByRole("alert")).toHaveTextContent(
      "Path is not in a directory Message Crate made in the Staging Directory",
    );
  });

  it("shows its own sentence when opening fails with an empty message", async () => {
    const user = setupUser();
    openPathInExplorer.mockRejectedValue(new Error(""));
    render(<OpenPathButton path="/tmp/nope">Open</OpenPathButton>);
    await user.click(screen.getByRole("button", { name: "Open" }));
    expect(screen.getByRole("alert")).toHaveTextContent(/^Could not open path$/);
  });

  it("shows the text a desktop command rejected with", async () => {
    const user = setupUser();
    openPathInExplorer.mockRejectedValue("No such file or directory");
    render(<OpenPathButton path="/tmp/nope">Open</OpenPathButton>);
    await user.click(screen.getByRole("button", { name: "Open" }));
    expect(screen.getByRole("alert")).toHaveTextContent(/^No such file or directory$/);
  });
});
