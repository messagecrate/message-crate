/** @vitest-environment jsdom */

import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { imessageMessage } from "../../test/apiShapes";
import ImessageBubble from "./ImessageBubble";

afterEach(() => {
  cleanup();
});

describe("ImessageBubble tapbacks", () => {
  it("renders a tapback as an emoji with a count", () => {
    render(
      <ImessageBubble
        message={imessageMessage({
          tapbacks: [
            { emoji: null, is_from_me: false, kind: "loved", part_index: 0, sender: "+1555" },
          ],
        })}
      />,
    );

    expect(screen.getByText("❤️ 1")).toBeInTheDocument();
  });

  it("renders nothing tapback-related for a message with none", () => {
    render(<ImessageBubble message={imessageMessage()} />);

    expect(screen.queryByText(/❤️|👍|👎|😂|‼️|❓/)).not.toBeInTheDocument();
  });
});
