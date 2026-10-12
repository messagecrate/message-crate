import { describe, expect, it } from "vitest";
import { ApiError } from "./api";
import { apiErrorMessage, errorText, rejectionMessage, searchErrorLine } from "./apiErrorMessage";

describe("apiErrorMessage", () => {
  it("gives the message of an Error", () => {
    expect(apiErrorMessage(new Error("Server refused it."), "Fallback.")).toBe(
      "Server refused it.",
    );
  });

  it("gives the fallback for an Error whose message is empty", () => {
    expect(apiErrorMessage(new Error(""), "Could not load that address book.")).toBe(
      "Could not load that address book.",
    );
  });

  it("gives the fallback for anything that is not an Error", () => {
    expect(apiErrorMessage("text", "Fallback.")).toBe("Fallback.");
  });
});

describe("rejectionMessage", () => {
  it("gives the string a desktop command rejected with", () => {
    expect(rejectionMessage("No such file or directory", "Fallback.")).toBe(
      "No such file or directory",
    );
  });

  it("gives the fallback for an empty string or an Error whose message is empty", () => {
    expect(rejectionMessage("", "Fallback.")).toBe("Fallback.");
    expect(rejectionMessage(new Error(""), "Fallback.")).toBe("Fallback.");
  });
});

describe("errorText", () => {
  it("gives the String form of an Error whose message is empty", () => {
    expect(errorText(new Error(""))).toBe("Error");
  });
});

/** A `422` refusal of problem type `slug`, as `apiClient` rejects with it. */
function refusal(slug: string, detail: string): ApiError {
  return new ApiError(422, detail, {
    type: `https://messagecrate.app/docs/developer/reference/errors/${slug}`,
    title: "Refused",
    status: 422,
    detail,
  });
}

describe("searchErrorLine", () => {
  it("starts a refusal that is not about one word with a capital", () => {
    expect(searchErrorLine(new Error("the search has too many parts"), "Fallback.")).toBe(
      "The search has too many parts",
    );
  });

  it("keeps a refusal that starts with a search word as the word is typed", () => {
    expect(
      searchErrorLine(
        refusal("search-query-invalid", "from: is not a Contacts word; it works on Messages"),
        "F.",
      ),
    ).toBe("from: is not a Contacts word; it works on Messages");
    expect(
      searchErrorLine(refusal("search-query-invalid", "first-message: needs a value"), "F."),
    ).toBe("first-message: needs a value");
  });

  it("starts any other refusal that names a parameter first with a capital", () => {
    const sort = refusal("validation-failed", "sort: unknown key 'size'");
    expect(searchErrorLine(sort, "F.")).toBe("Sort: unknown key 'size'");
    expect(searchErrorLine(new Error("from: is not a word"), "F.")).toBe("From: is not a word");
  });

  it("gives the fallback for anything that is not an Error", () => {
    expect(searchErrorLine(null, "Could not load conversations.")).toBe(
      "Could not load conversations.",
    );
  });
});
