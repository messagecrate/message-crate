/** @vitest-environment jsdom */

import { cleanup, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { renderWithProviders } from "../test/providers";
import SourcesPanel from "./SourcesPanel";

vi.mock("../lib/authContext", () => ({ useAuth: () => ({ accountId: 7 }) }));

const getSources = vi.fn();

vi.mock("../lib/serverApi", () => ({
  getConversationSources: (...args: unknown[]) => getSources(...args),
}));

function renderPanel() {
  renderWithProviders(<SourcesPanel conversationId={1} onClose={() => {}} />);
}

describe("SourcesPanel", () => {
  afterEach(() => {
    cleanup();
    getSources.mockReset();
  });

  it("puts each share of unique messages beside the unique count it was computed from", async () => {
    // Two sources with 2 messages each; one message is in both, so the first
    // source holds 2 of the 3 unique messages and the second holds 1.
    getSources.mockResolvedValue([
      { backup_name: "phone-a", message_count: 2, unique_count: 2, percentage: 66.7 },
      { backup_name: "phone-b", message_count: 2, unique_count: 1, percentage: 33.3 },
    ]);
    renderPanel();

    expect(await screen.findByText("2 unique (66.7% of unique messages)")).toBeTruthy();
    expect(screen.getByText("1 unique (33.3% of unique messages)")).toBeTruthy();
    expect(screen.getAllByText("2 messages")).toHaveLength(2);
  });

  it("says there is no source data, without a full stop, when the conversation has none", async () => {
    getSources.mockResolvedValue([]);
    renderPanel();

    expect(await screen.findByText("No source data available")).toBeTruthy();
  });
});
