/** @vitest-environment jsdom */

import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
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
});
