import { describe, expect, it } from "vitest";
import { countOf } from "./plural";

describe("countOf", () => {
  it("writes one with the noun as it is", () => {
    expect(countOf(1, "message")).toBe("1 message");
  });

  it("adds s for any other count", () => {
    expect(countOf(0, "conversation")).toBe("0 conversations");
    expect(countOf(2, "contact")).toBe("2 contacts");
  });

  it("takes an irregular plural", () => {
    expect(countOf(1, "identity", "identities")).toBe("1 identity");
    expect(countOf(3, "identity", "identities")).toBe("3 identities");
  });

  it("writes the number the way the locale does", () => {
    expect(countOf(1234, "message")).toBe(`${(1234).toLocaleString()} messages`);
  });
});
