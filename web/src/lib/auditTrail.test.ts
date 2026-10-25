import { describe, expect, it } from "vitest";
import { auditEntry } from "../test/apiShapes";
import { type AuditEntry, describeAuditEntry } from "./auditTrail";

/** An entry with only who, what and when; each test adds what it describes. */
function entry(fields: Partial<AuditEntry> & Pick<AuditEntry, "action">): AuditEntry {
  return auditEntry({ account_id: 101, username: "alice", ...fields });
}

describe("describeAuditEntry", () => {
  it("names the contacts a change to contacts records by id, never by name", () => {
    expect(describeAuditEntry(entry({ action: "contact_added", contact_id: 12 }))).toBe(
      "Contact added by hand (id 12)",
    );
    expect(
      describeAuditEntry(
        entry({ action: "contacts_trashed", contacts: 3, contact_ids: [3, 5, 9] }),
      ),
    ).toBe("Moved 3 contacts to Trash (ids 3, 5 and 9)");
    expect(
      describeAuditEntry(entry({ action: "contacts_restored", contacts: 1, contact_ids: [3] })),
    ).toBe("Restored 1 contact from Trash (id 3)");
    expect(describeAuditEntry(entry({ action: "contact_deleted", contact_id: 4 }))).toBe(
      "A contact deleted for good from the Trash (id 4)",
    );
    expect(
      describeAuditEntry(
        entry({ action: "contacts_merged", contact_id: 5, contacts: 2, contact_ids: [6, 7] }),
      ),
    ).toBe("Merged 2 contacts into contact 5 (ids 6 and 7)");
    expect(
      describeAuditEntry(
        entry({ action: "contacts_unmerged", contact_id: 5, contacts: 2, contact_ids: [6, 7] }),
      ),
    ).toBe("Unmerged 2 contacts from contact 5 (ids 6 and 7)");
  });

  it("names the first ten contacts of a large move and counts the rest", () => {
    const ids = Array.from({ length: 12 }, (_, i) => i + 1);
    expect(
      describeAuditEntry(entry({ action: "contacts_trashed", contacts: 12, contact_ids: ids })),
    ).toBe("Moved 12 contacts to Trash (ids 1, 2, 3, 4, 5, 6, 7, 8, 9, 10 and 2 more)");
  });

  it("says how a Session ended", () => {
    expect(describeAuditEntry(entry({ action: "session_ended", reason: "replaced" }))).toBe(
      "Session ended by a newer login",
    );
    expect(describeAuditEntry(entry({ action: "session_ended", reason: "expired" }))).toBe(
      "Session expired",
    );
  });

  it("says why the server canceled an Export Run", () => {
    expect(
      describeAuditEntry(
        entry({ action: "export_run_closed", reason: "replaced", export_run_id: 12 }),
      ),
    ).toBe("Export Run 12 canceled: a new Export started");
    expect(
      describeAuditEntry(entry({ action: "export_run_closed", reason: "idle", export_run_id: 12 })),
    ).toBe("Export Run 12 canceled: no page was read for an hour");
  });

  it("names the username a refused login typed when no account has it", () => {
    expect(
      describeAuditEntry(
        entry({
          action: "login_refused",
          reason: "unknown_username",
          account_id: null,
          username: "nobody",
          app: "desktop",
          app_build: "0.10.0+bbbb2222",
        }),
      ),
    ).toBe("Login refused: no account is named “nobody” from the desktop app (0.10.0+bbbb2222)");
  });

  it("says a refused login typed no valid username when the server kept none", () => {
    expect(
      describeAuditEntry(
        entry({
          action: "login_refused",
          reason: "unknown_username",
          account_id: null,
          username: null,
        }),
      ),
    ).toBe("Login refused: not a valid username");
  });

  it("names the API token that started a run, by label and hint", () => {
    expect(
      describeAuditEntry(
        entry({
          action: "export_run",
          status: "completed",
          scope_kind: "query",
          scope_list: "messages",
          messages: 1,
          conversations: 2,
          credential: "api_token",
          api_token_label: "nightly backup",
          api_token_hint: "mc-api-Sd..mE",
        }),
      ),
    ).toBe(
      "Export Run of a search of messages, completed: 1 message in 2 conversations with the API token “nightly backup” (mc-api-Sd..mE)",
    );
  });

  it("lists the permissions turned on and off", () => {
    expect(
      describeAuditEntry(
        entry({
          action: "permissions_changed",
          actor: "owner",
          permissions_added: ["import"],
          permissions_removed: ["export", "delete"],
        }),
      ),
    ).toBe("Permissions changed: allowed import; removed export and delete");
  });

  it("names the attachment size limit the owner set", () => {
    expect(
      describeAuditEntry(
        auditEntry({ action: "asset_limit_changed", actor: "owner", asset_max_bytes: 104857600 }),
      ),
    ).toBe("Attachment size limit set to 100 MB");
    expect(describeAuditEntry(auditEntry({ action: "asset_limit_changed", actor: "owner" }))).toBe(
      "Attachment size limit changed",
    );
  });

  it("names the set of Demo Data the owner added or reset the Demo Account with", () => {
    expect(
      describeAuditEntry(
        auditEntry({ action: "demo_account_added", actor: "owner", demo_data_size: "medium" }),
      ),
    ).toBe("Demo Account added with the medium set");
    expect(
      describeAuditEntry(
        auditEntry({
          action: "demo_account_reset",
          actor: "owner",
          username: "demo",
          demo_data_size: "large",
        }),
      ),
    ).toBe("Demo Account reset with the large set");
    expect(
      describeAuditEntry(
        auditEntry({ action: "demo_account_reset", actor: "owner", username: "demo" }),
      ),
    ).toBe("Demo Account reset");
  });
});
