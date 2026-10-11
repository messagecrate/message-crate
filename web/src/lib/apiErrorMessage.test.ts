import { describe, expect, it } from "vitest";
import { apiErrorMessage, errorText, rejectionMessage } from "./apiErrorMessage";

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
