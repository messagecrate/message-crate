/** @vitest-environment jsdom */

import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { setupUser } from "../test/user";
import GroupNameDialog from "./GroupNameDialog";

afterEach(cleanup);

describe("GroupNameDialog", () => {
  it("stays open on Escape while its save is in flight", async () => {
    const user = setupUser();
    const onCancel = vi.fn();
    render(
      <GroupNameDialog
        title="New Contact Group"
        initial="Family"
        busy
        onSave={() => {}}
        onCancel={onCancel}
      />,
    );

    await user.keyboard("{Escape}");

    expect(screen.getByRole("dialog", { name: "New Contact Group" })).toBeInTheDocument();
    expect(onCancel).not.toHaveBeenCalled();
  });

  it("closes on Escape when no save is in flight", async () => {
    const user = setupUser();
    const onCancel = vi.fn();
    render(<GroupNameDialog title="New Contact Group" onSave={() => {}} onCancel={onCancel} />);

    await user.keyboard("{Escape}");

    expect(onCancel).toHaveBeenCalledTimes(1);
  });
});
