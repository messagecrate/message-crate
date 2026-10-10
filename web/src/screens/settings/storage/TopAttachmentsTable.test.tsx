/** @vitest-environment jsdom */

import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import TopAttachmentsTable from "./TopAttachmentsTable";

afterEach(cleanup);

describe("TopAttachmentsTable", () => {
  it("says when no attachment has a size", () => {
    render(
      <TopAttachmentsTable
        topAttachments={[]}
        page={0}
        onPageChange={() => {}}
        showConversation={true}
      />,
    );
    expect(screen.getByText("No attachments with sizes yet")).toBeInTheDocument();
  });
});
