import { describe, expect, it } from "vitest";
import { contactBrowseQuery } from "./contactBrowseQuery";

describe("contactBrowseQuery", () => {
  it("builds a bare handle term when the handle needs no quoting", () => {
    expect(contactBrowseQuery({ contactId: "42", kind: "all", handle: "ann@example.com" })).toBe(
      "identity:ann@example.com",
    );
  });

  it("quotes a handle with a space", () => {
    expect(contactBrowseQuery({ contactId: "42", kind: "all", handle: "Ann Lee" })).toBe(
      'identity:"Ann Lee"',
    );
  });

  it("quotes a handle with parentheses, since the language reads them as grouping", () => {
    expect(contactBrowseQuery({ contactId: "42", kind: "all", handle: "Ann (Lee)" })).toBe(
      'identity:"Ann (Lee)"',
    );
  });

  it("falls back to the contact id when there is no handle", () => {
    expect(contactBrowseQuery({ contactId: "42", kind: "all" })).toBe("with:#42");
    expect(contactBrowseQuery({ contactId: "42", kind: "all", handle: "" })).toBe("with:#42");
    expect(contactBrowseQuery({ contactId: "42", kind: "all", handle: "   " })).toBe("with:#42");
  });

  it("narrows to kind:direct or kind:group, and leaves 'all' as the base query", () => {
    expect(contactBrowseQuery({ contactId: "42", kind: "direct" })).toBe("kind:direct (with:#42)");
    expect(contactBrowseQuery({ contactId: "42", kind: "group" })).toBe("kind:group (with:#42)");
    expect(contactBrowseQuery({ contactId: "42", kind: "all" })).toBe("with:#42");
    expect(contactBrowseQuery({ contactId: "42", kind: "direct", handle: "Ann Lee" })).toBe(
      'kind:direct (identity:"Ann Lee")',
    );
  });
});
