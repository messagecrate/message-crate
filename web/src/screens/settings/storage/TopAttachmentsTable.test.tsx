/** @vitest-environment jsdom */

import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import type { TopAttachment } from "./storageUtils";
import TopAttachmentsTable from "./TopAttachmentsTable";

function attachment(id: number): TopAttachment {
  return {
    id,
    chat_identifier: null,
    conversation_id: null,
    group_title: null,
    mime_type: "image/jpeg",
    original_name: `photo-${id}.jpg`,
    size_bytes: 1000 * id,
  };
}

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
    expect(screen.queryByText(/attachments by file size/)).toBeNull();
  });

  it("gives the number of attachments the server sent", () => {
    render(
      <TopAttachmentsTable
        topAttachments={[attachment(1), attachment(2), attachment(3)]}
        page={0}
        onPageChange={() => {}}
        showConversation={false}
      />,
    );
    expect(screen.getByText("Top 3 attachments by file size.")).toBeInTheDocument();
  });
});
