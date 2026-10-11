/** @vitest-environment jsdom */

import { cleanup, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { auditEntry } from "../../test/apiShapes";
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
          auditEntry({ id: 2, action: "asset_limit_changed", actor: "owner", username: "dave" }),
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
    expect(within(rows[1]).getByText("carol")).toBeInTheDocument();
    expect(within(rows[1]).getByText("deleted")).toBeInTheDocument();
    expect(within(rows[2]).getByText("dave")).toBeInTheDocument();
    expect(within(rows[2]).queryByText("deleted")).toBeNull();
    expect(within(rows[3]).getByText("nobody")).toBeInTheDocument();
    expect(within(rows[3]).queryByText("deleted")).toBeNull();
  });
});
