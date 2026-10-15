/** @vitest-environment jsdom */

import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { setupUser } from "../../test/user";
import MessageFindBar from "./MessageFindBar";

/** The bar with a term typed, spies for its actions, and `overrides` on top. */
function renderBar(
  overrides: Partial<{ matchCount: number; matchPosition: number; searching: boolean }> = {},
) {
  const onPrevMatch = vi.fn();
  const onNextMatch = vi.fn();
  const onClose = vi.fn();
  render(
    <MessageFindBar
      findTerm="lunch"
      onFindTermChange={() => {}}
      matchCount={3}
      matchPosition={0}
      searching={false}
      onPrevMatch={onPrevMatch}
      onNextMatch={onNextMatch}
      onClose={onClose}
      {...overrides}
    />,
  );
  return { onPrevMatch, onNextMatch, onClose };
}

describe("MessageFindBar", () => {
  it("counts the current match from one", () => {
    renderBar({ matchCount: 3, matchPosition: 0 });
    expect(screen.getByText("1 of 3")).toBeInTheDocument();
  });

  it("counts a later match from its position", () => {
    renderBar({ matchCount: 3, matchPosition: 1 });
    expect(screen.getByText("2 of 3")).toBeInTheDocument();
  });

  it("steps to the older match on Enter and the newer one on Shift+Enter", async () => {
    const user = setupUser();
    const { onPrevMatch, onNextMatch } = renderBar();
    // The bar takes focus when it opens, so the keys go to its field.
    expect(document.activeElement).toBe(
      screen.getByRole("textbox", { name: "Find in conversation" }),
    );

    await user.keyboard("{Enter}");
    expect(onPrevMatch).toHaveBeenCalledTimes(1);
    expect(onNextMatch).not.toHaveBeenCalled();

    await user.keyboard("{Shift>}{Enter}{/Shift}");
    expect(onNextMatch).toHaveBeenCalledTimes(1);
    expect(onPrevMatch).toHaveBeenCalledTimes(1);
  });

  it("steps with the older and newer match buttons", async () => {
    const user = setupUser();
    const { onPrevMatch, onNextMatch } = renderBar();

    await user.click(screen.getByRole("button", { name: "Older match" }));
    expect(onPrevMatch).toHaveBeenCalledTimes(1);
    await user.click(screen.getByRole("button", { name: "Newer match" }));
    expect(onNextMatch).toHaveBeenCalledTimes(1);
  });

  it("steps nowhere when nothing matches", async () => {
    const user = setupUser();
    const { onPrevMatch, onNextMatch } = renderBar({ matchCount: 0 });

    await user.keyboard("{Enter}");
    await user.keyboard("{Shift>}{Enter}{/Shift}");
    expect(onPrevMatch).not.toHaveBeenCalled();
    expect(onNextMatch).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "Older match" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Newer match" })).toBeDisabled();
    expect(screen.getByText("No matches")).toBeInTheDocument();
  });

  it("says it is finding while the matches are still being read", () => {
    renderBar({ matchCount: 0, searching: true });
    expect(screen.getByText("Finding…")).toBeInTheDocument();
    expect(screen.queryByText("No matches")).not.toBeInTheDocument();
  });

  it("closes on Escape", async () => {
    const user = setupUser();
    const { onClose } = renderBar();

    await user.keyboard("{Escape}");
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});
