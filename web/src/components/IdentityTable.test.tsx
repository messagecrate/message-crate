/** @vitest-environment jsdom */

import { cleanup, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { inTimeZone } from "../test/timeZone";
import { setupUser } from "../test/user";
import IdentityTable, { type IdentityRow } from "./IdentityTable";

afterEach(cleanup);

const rows: IdentityRow[] = [
  {
    address: "+15555550100",
    service: "phone",
    start_date: "2020-01-01T12:00:00Z",
    end_date: "2020-02-03T12:00:00Z",
    conversations: 2,
    direct_messages: 12,
    group_messages: 30,
    orphaned_messages: 0,
  },
  {
    address: "someone.with.a.long.address@example.com",
    service: "email",
    start_date: null,
    end_date: null,
    conversations: 0,
    direct_messages: 0,
    group_messages: 0,
    orphaned_messages: 0,
  },
  {
    address: "+15555550100",
    service: "whatsapp",
    start_date: "2021-05-05T12:00:00Z",
    end_date: "2021-05-06T12:00:00Z",
    conversations: 1,
    direct_messages: 3,
    group_messages: 0,
    orphaned_messages: 0,
  },
];

// The table has no date headings of its own: each screen names the two dates.
const dates = { firstDateHeading: "First heard from", lastDateHeading: "Last heard from" };

const headers = () =>
  screen.getAllByRole("columnheader").map((h) => h.textContent?.replace(/[▲▼]/g, "").trim());
const identities = () =>
  screen.getAllByRole("rowheader").map((cell) => cell.textContent?.trim() ?? "");

describe("IdentityTable", () => {
  it("shows the eight columns, text left and numbers right, with the header aligned like its cells", () => {
    render(inTimeZone("UTC", <IdentityTable {...dates} rows={rows} onRemove={() => {}} />));

    expect(headers()).toEqual([
      "Service",
      "Identity",
      "First heard from",
      "Last heard from",
      "Conversations",
      "Direct messages",
      "Group messages",
      "",
    ]);
    const [service, identity, first, , conversations] = screen.getAllByRole("columnheader");
    expect(service.className).toContain("text-left");
    expect(identity.className).toContain("text-left");
    expect(first.className).toContain("text-right");
    expect(conversations.className).toContain("text-right");

    const row = screen.getAllByRole("row")[1];
    const cells = within(row).getAllByRole("gridcell");
    expect(cells[0].className).toContain("text-left");
    expect(cells[1].className).toContain("text-right");
    expect(cells[3].className).toContain("text-right");
    expect(cells[1].textContent).toBe("2020-01-01");
    expect(cells[3].textContent).toBe("2");
    expect(cells[5].textContent).toBe("30");
  });

  it("shows an Orphaned messages column, summed in the Summary row, when an identity has orphaned messages", () => {
    const withOrphaned = rows.map((row, i) => (i === 2 ? { ...row, orphaned_messages: 4 } : row));
    render(
      inTimeZone(
        "UTC",
        <IdentityTable {...dates} rows={withOrphaned} totalConversations={4} onRemove={() => {}} />,
      ),
    );

    expect(headers()).toEqual([
      "Service",
      "Identity",
      "First heard from",
      "Last heard from",
      "Conversations",
      "Direct messages",
      "Group messages",
      "Orphaned messages",
      "",
    ]);
    const whatsapp = screen
      .getAllByRole("row")
      .find((row) => row.textContent?.includes("2021-05-05"));
    expect(whatsapp).toBeDefined();
    expect(within(whatsapp as HTMLElement).getAllByRole("gridcell")[6].textContent).toBe("4");
    const summary = screen.getAllByRole("row").at(-1) as HTMLElement;
    expect(within(summary).getAllByRole("gridcell")[6].textContent).toBe("4");
  });

  it("leaves the Orphaned messages column out when no identity has orphaned messages", () => {
    render(
      inTimeZone(
        "UTC",
        <IdentityTable {...dates} rows={rows} totalConversations={3} onRemove={() => {}} />,
      ),
    );

    expect(headers()).not.toContain("Orphaned messages");
    expect(screen.queryByText(/orphaned/i)).toBeNull();
  });

  it("suspends a sort by Orphaned messages while that column is gone", async () => {
    const user = setupUser();
    const withOrphaned = rows.map((row, i) => (i === 1 ? { ...row, orphaned_messages: 4 } : row));
    const { rerender } = render(<IdentityTable {...dates} rows={withOrphaned} />);

    await user.click(screen.getByRole("columnheader", { name: /Orphaned messages/ }));
    expect(identities()[2]).toBe("someone.with.a.long.address@example.com");

    // No identity has orphaned messages any more, so the column goes, and the
    // rows return to the order the caller gave rather than a hidden column's.
    rerender(<IdentityTable {...dates} rows={rows} />);
    expect(headers()).not.toContain("Orphaned messages");
    expect(identities()).toEqual([
      "+15555550100",
      "someone.with.a.long.address@example.com",
      "+15555550100",
    ]);

    // When the column comes back, so does the sort the person chose: the
    // column can go for a moment while figures reload, and that must not undo
    // their choice.
    rerender(<IdentityTable {...dates} rows={withOrphaned} />);
    expect(
      screen.getByRole("columnheader", { name: /Orphaned messages/ }).getAttribute("aria-sort"),
    ).not.toBe("none");
    expect(identities()[2]).toBe("someone.with.a.long.address@example.com");
  });

  it("puts the sort arrow right after the label and shows it only on the sorted column", async () => {
    const user = setupUser();
    render(<IdentityTable {...dates} rows={rows} onRemove={() => {}} />);

    const identity = screen.getByRole("columnheader", { name: /^Identity/ });
    const arrow = identity.querySelector("[aria-hidden]");
    expect(arrow?.className).toContain("invisible");
    expect(arrow?.previousSibling?.textContent).toBe("Identity");

    await user.click(identity);
    expect(identity.querySelector("[aria-hidden]")?.className).not.toContain("invisible");
    expect(identities()).toEqual([
      "+15555550100",
      "+15555550100",
      "someone.with.a.long.address@example.com",
    ]);

    await user.click(identity);
    expect(identities()[0]).toBe("someone.with.a.long.address@example.com");

    await user.click(screen.getByRole("columnheader", { name: /Direct messages/ }));
    expect(identities()[0]).toBe("someone.with.a.long.address@example.com");
    expect(identity.querySelector("[aria-hidden]")?.className).toContain("invisible");
  });

  it("cuts a long identity with an ellipsis and keeps the whole value in the title", () => {
    render(<IdentityTable {...dates} rows={rows} onRemove={() => {}} />);
    const cell = screen.getByRole("rowheader", { name: /someone/ });
    const text = within(cell).getByTitle("someone.with.a.long.address@example.com");
    expect(text.className).toContain("truncate");
  });

  it("shows a muted dash for a zero count or a missing date", () => {
    render(<IdentityTable {...dates} rows={rows} onRemove={() => {}} />);
    const row = screen.getAllByRole("row")[2];
    const cells = within(row).getAllByRole("gridcell");
    expect(cells[1].textContent).toBe("—");
    expect(cells[3].textContent).toBe("—");
    expect(cells[5].textContent).toBe("—");
  });

  it("ends every row with an always visible Remove named after the identity and its service", async () => {
    const user = setupUser();
    const onRemove = vi.fn();
    render(<IdentityTable {...dates} rows={rows} onRemove={onRemove} />);

    const remove = screen.getByRole("button", {
      name: "Remove someone.with.a.long.address@example.com (Email)",
    });
    expect(remove.className).not.toMatch(/opacity-0/);
    const row = remove.closest("[role=row]");
    expect(row?.lastElementChild).toContainElement(remove);

    await user.click(remove);
    expect(onRemove).toHaveBeenCalledWith(rows[1]);
  });

  it("disables Remove while busy", () => {
    render(<IdentityTable {...dates} rows={rows} busy onRemove={() => {}} />);
    expect(
      screen.getByRole("button", {
        name: "Remove someone.with.a.long.address@example.com (Email)",
      }),
    ).toBeDisabled();
  });

  it("makes the conversation count a link only when given somewhere to browse to", async () => {
    const user = setupUser();
    const { unmount } = render(<IdentityTable {...dates} rows={rows} onRemove={() => {}} />);
    expect(screen.queryByRole("button", { name: /Open 2 conversations/ })).not.toBeInTheDocument();
    unmount();

    const onBrowse = vi.fn();
    render(<IdentityTable {...dates} rows={rows} onRemove={() => {}} onBrowse={onBrowse} />);
    await user.click(screen.getByRole("button", { name: "Open 2 conversations" }));
    expect(onBrowse).toHaveBeenCalledWith(rows[0]);
    // A zero count is never a link.
    expect(screen.queryByRole("button", { name: /Open 0/ })).not.toBeInTheDocument();
  });

  it("adds a Summary row only when asked, with the earliest, latest, message sums and the conversations it is given", () => {
    const { unmount } = render(<IdentityTable {...dates} rows={rows} onRemove={() => {}} />);
    expect(screen.queryByText("Summary")).not.toBeInTheDocument();
    unmount();

    // The rows' conversations add up to 3; two identities share one, so the
    // contact has 2.
    render(
      inTimeZone(
        "UTC",
        <IdentityTable {...dates} rows={rows} totalConversations={2} onRemove={() => {}} />,
      ),
    );
    const summary = screen.getByText("Summary").closest("[role=row]");
    const cells = within(summary as HTMLElement).getAllByRole("gridcell");
    expect(cells.map((c) => c.textContent)).toEqual([
      "Summary",
      "2020-01-01",
      "2021-05-06",
      "2",
      "15",
      "30",
      "",
    ]);
  });

  it("shows dashes for every count and date while loading", () => {
    render(<IdentityTable {...dates} rows={rows} loading onRemove={() => {}} />);
    const row = screen.getAllByRole("row")[1];
    const cells = within(row).getAllByRole("gridcell");
    expect(cells.slice(1, 6).map((c) => c.textContent)).toEqual(["—", "—", "—", "—", "—"]);
    expect(
      screen.getByRole("button", { name: "Remove +15555550100 (Text Message)" }),
    ).toBeDisabled();
  });

  it("says so instead of drawing a table when there are no identities", () => {
    render(
      <IdentityTable {...dates} rows={[]} onRemove={() => {}} emptyText="No identities yet" />,
    );
    expect(screen.queryByRole("grid")).not.toBeInTheDocument();
    expect(screen.getByText("No identities yet")).toBeInTheDocument();
  });
});

describe("IdentityTable dates", () => {
  it("shows the day in the account's time zone, as the contact list does", () => {
    // 20:00 on 31 December 2024 in Los Angeles.
    const late: IdentityRow = {
      address: "+15555550100",
      service: "phone",
      start_date: "2025-01-01T04:00:00Z",
      end_date: "2025-01-01T04:00:00Z",
      conversations: 1,
      direct_messages: 1,
      group_messages: 0,
      orphaned_messages: 0,
    };
    render(
      inTimeZone(
        "America/Los_Angeles",
        <IdentityTable {...dates} rows={[late]} onRemove={() => {}} />,
      ),
    );
    expect(screen.getAllByText("2024-12-31")).toHaveLength(2);
  });
});

describe("IdentityTable focus", () => {
  // The app's own :focus-visible outline loses to the outline-none utility, so
  // a sortable header draws the style guide's ring itself.
  it("draws the focus ring on every sortable column header", () => {
    render(<IdentityTable {...dates} rows={rows} onRemove={() => {}} />);
    const sortable = screen
      .getAllByRole("columnheader")
      .filter((header) => header.hasAttribute("aria-sort"));
    expect(sortable).toHaveLength(7);
    for (const header of sortable) {
      expect(header.className).toContain("focus-visible:ring-2 focus-visible:ring-accent");
    }
  });

  it("draws the focus ring on every row, the summary row included", () => {
    render(<IdentityTable {...dates} rows={rows} totalConversations={2} onRemove={() => {}} />);
    const bodyRows = screen.getAllByRole("row").slice(1);
    expect(bodyRows).toHaveLength(4);
    expect(bodyRows.some((row) => row.textContent?.startsWith("Summary"))).toBe(true);
    for (const row of bodyRows) {
      expect(row.className).toContain(
        "focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-accent",
      );
    }
  });
});
