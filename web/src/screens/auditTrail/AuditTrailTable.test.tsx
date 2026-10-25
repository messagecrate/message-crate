/** @vitest-environment jsdom */

import { cleanup, render, screen, within } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { afterEach, describe, expect, it } from "vitest";
import { auditEntry } from "../../test/apiShapes";
import { nth } from "../../test/nth";
import AuditTrailTable from "./AuditTrailTable";

afterEach(cleanup);

describe("AuditTrailTable", () => {
  it("says when the Audit Trail holds no entry", () => {
    render(
      <AuditTrailTable
        entries={[]}
        total={0}
        page={0}
        onPageChange={() => {}}
        showAccount={false}
      />,
    );
    expect(screen.getByText("Nothing recorded yet")).toBeInTheDocument();
  });

  it("marks an account deleted only when the server says so", () => {
    render(
      <AuditTrailTable
        entries={[
          auditEntry({
            id: 3,
            action: "account_deleted",
            username: "carol",
            account_deleted: true,
          }),
          // About no account, yet carrying a username: not a deleted account.
          auditEntry({ id: 2, action: "asset_limit_changed", actor: "admin", username: "dave" }),
          auditEntry({
            id: 1,
            action: "login_refused",
            actor: "server",
            username: "nobody",
            reason: "unknown_username",
          }),
        ]}
        total={3}
        page={0}
        onPageChange={() => {}}
        showAccount
      />,
    );
    const rows = screen.getAllByRole("row");
    expect(within(nth(rows, 1)).getByText("carol")).toBeInTheDocument();
    expect(within(nth(rows, 1)).getByText("deleted")).toBeInTheDocument();
    expect(within(nth(rows, 2)).getByText("dave")).toBeInTheDocument();
    expect(within(nth(rows, 2)).queryByText("deleted")).toBeNull();
    expect(within(nth(rows, 3)).getByText("nobody")).toBeInTheDocument();
    expect(within(nth(rows, 3)).queryByText("deleted")).toBeNull();
  });

  it("links an entry to its request's lines in the log only when asked, and only when it has a request", () => {
    const entries = [
      auditEntry({ id: 2, action: "logged_in", request_id: "7f3a9c21" }),
      // Written by a server command, outside any request.
      auditEntry({ id: 1, action: "asset_limit_changed", actor: "admin", request_id: null }),
    ];
    const { unmount } = render(
      <MemoryRouter>
        <AuditTrailTable
          entries={entries}
          total={2}
          page={0}
          onPageChange={() => {}}
          showAccount
          logHref={(requestId) => `/admin/logs?request=${requestId}`}
        />
      </MemoryRouter>,
    );
    const rows = screen.getAllByRole("row");
    expect(within(nth(rows, 1)).getByRole("link", { name: "View in log" })).toHaveAttribute(
      "href",
      "/admin/logs?request=7f3a9c21",
    );
    expect(within(nth(rows, 2)).queryByRole("link")).toBeNull();
    unmount();

    // The holder's own Audit Trail in Settings: a holder cannot read the log.
    render(
      <AuditTrailTable
        entries={entries}
        total={2}
        page={0}
        onPageChange={() => {}}
        showAccount={false}
      />,
    );
    expect(screen.queryByRole("link")).toBeNull();
  });
});
