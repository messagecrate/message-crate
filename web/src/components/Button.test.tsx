/** @vitest-environment jsdom */

import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { setupUser } from "../test/user";
import Button from "./Button";

describe("Button", () => {
  it("invokes onPress when clicked", async () => {
    const user = setupUser();
    const onPress = vi.fn();
    render(
      <Button variant="primary" onPress={onPress}>
        Save
      </Button>,
    );
    await user.click(screen.getByRole("button", { name: "Save" }));
    expect(onPress).toHaveBeenCalledTimes(1);
  });
});
